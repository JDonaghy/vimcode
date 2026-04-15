use super::*;

impl Engine {
    // --- Word motions ---

    pub(crate) fn move_word_forward(&mut self) {
        let total_chars = self.buffer().len_chars();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos >= total_chars {
            return;
        }

        let first = self.buffer().content.char(pos);
        if is_word_char(first) {
            while pos < total_chars && is_word_char(self.buffer().content.char(pos)) {
                pos += 1;
            }
        } else if !first.is_whitespace() {
            while pos < total_chars {
                let ch = self.buffer().content.char(pos);
                if is_word_char(ch) || ch.is_whitespace() {
                    break;
                }
                pos += 1;
            }
        }

        while pos < total_chars && self.buffer().content.char(pos).is_whitespace() {
            pos += 1;
        }

        if pos >= total_chars {
            pos = total_chars.saturating_sub(1);
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    pub(crate) fn move_word_backward(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos == 0 {
            return;
        }
        pos -= 1;

        while pos > 0 && self.buffer().content.char(pos).is_whitespace() {
            pos -= 1;
        }

        let ch = self.buffer().content.char(pos);
        if is_word_char(ch) {
            while pos > 0 && is_word_char(self.buffer().content.char(pos - 1)) {
                pos -= 1;
            }
        } else {
            while pos > 0 {
                let prev = self.buffer().content.char(pos - 1);
                if is_word_char(prev) || prev.is_whitespace() {
                    break;
                }
                pos -= 1;
            }
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    pub(crate) fn move_word_end(&mut self) {
        let total_chars = self.buffer().len_chars();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos >= total_chars {
            return;
        }

        let current_char = self.buffer().content.char(pos);

        // Check if we're already at the end of a word
        let at_word_end = if pos + 1 < total_chars {
            let next_char = self.buffer().content.char(pos + 1);
            (is_word_char(current_char) && !is_word_char(next_char))
                || (!is_word_char(current_char)
                    && !current_char.is_whitespace()
                    && (is_word_char(next_char) || next_char.is_whitespace()))
        } else {
            false
        };

        // If at end of word, move to next word; otherwise move within current word
        if at_word_end || current_char.is_whitespace() {
            // Skip past current position
            pos += 1;
            // Skip whitespace
            while pos < total_chars && self.buffer().content.char(pos).is_whitespace() {
                pos += 1;
            }
        } else {
            // We're in the middle of a word, find its end
            // Don't increment pos here - stay on current character
        }

        if pos >= total_chars {
            pos = total_chars - 1;
        }

        let ch = self.buffer().content.char(pos);
        if is_word_char(ch) {
            while pos + 1 < total_chars && is_word_char(self.buffer().content.char(pos + 1)) {
                pos += 1;
            }
        } else if !ch.is_whitespace() {
            while pos + 1 < total_chars {
                let next = self.buffer().content.char(pos + 1);
                if is_word_char(next) || next.is_whitespace() {
                    break;
                }
                pos += 1;
            }
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    pub(crate) fn move_word_end_backward(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos == 0 {
            return;
        }

        let ch = self.buffer().content.char(pos);

        // Step 1: If on a non-whitespace char, go to the start of the current word.
        // If on whitespace, just move back one to begin searching.
        if !ch.is_whitespace() {
            if is_word_char(ch) {
                while pos > 0 && is_word_char(self.buffer().content.char(pos - 1)) {
                    pos -= 1;
                }
            } else {
                while pos > 0 {
                    let prev = self.buffer().content.char(pos - 1);
                    if is_word_char(prev) || prev.is_whitespace() {
                        break;
                    }
                    pos -= 1;
                }
            }
        }

        if pos == 0 {
            // Already at start of first word — nowhere to go further back.
            // But Vim's `ge` from start of first word goes to col 0 (no-op if
            // already there). We're already there.
            let new_line = self.buffer().content.char_to_line(pos);
            let line_start = self.buffer().line_to_char(new_line);
            self.view_mut().cursor.line = new_line;
            self.view_mut().cursor.col = pos - line_start;
            return;
        }

        // Step 2: Move back one char (from start of current word or from whitespace)
        pos -= 1;

        // Step 3: Skip whitespace backward
        while pos > 0 && self.buffer().content.char(pos).is_whitespace() {
            pos -= 1;
        }

        // If at pos 0 and it's whitespace, no previous word exists
        if pos == 0 && self.buffer().content.char(pos).is_whitespace() {
            return;
        }

        // pos is now at the last char of the previous word (the target)
        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    // --- WORD motions (whitespace-delimited) ---

    pub(crate) fn move_bigword_forward(&mut self) {
        let total_chars = self.buffer().len_chars();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos >= total_chars {
            return;
        }

        // Skip current WORD (non-whitespace)
        while pos < total_chars && !self.buffer().content.char(pos).is_whitespace() {
            pos += 1;
        }
        // Skip whitespace
        while pos < total_chars && self.buffer().content.char(pos).is_whitespace() {
            pos += 1;
        }

        if pos >= total_chars {
            pos = total_chars.saturating_sub(1);
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    pub(crate) fn move_bigword_backward(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos == 0 {
            return;
        }
        pos -= 1;

        // Skip whitespace backward
        while pos > 0 && self.buffer().content.char(pos).is_whitespace() {
            pos -= 1;
        }

        // Skip WORD backward (non-whitespace)
        while pos > 0 && !self.buffer().content.char(pos - 1).is_whitespace() {
            pos -= 1;
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    pub(crate) fn move_bigword_end(&mut self) {
        let total_chars = self.buffer().len_chars();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos >= total_chars {
            return;
        }

        // If next char is whitespace or we're at end of WORD, advance first
        let at_end = pos + 1 >= total_chars || self.buffer().content.char(pos + 1).is_whitespace();
        if at_end || self.buffer().content.char(pos).is_whitespace() {
            pos += 1;
            while pos < total_chars && self.buffer().content.char(pos).is_whitespace() {
                pos += 1;
            }
        }

        if pos >= total_chars {
            pos = total_chars - 1;
        }

        // Move to end of current WORD
        while pos + 1 < total_chars && !self.buffer().content.char(pos + 1).is_whitespace() {
            pos += 1;
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    pub(crate) fn move_bigword_end_backward(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos == 0 {
            return;
        }

        let ch = self.buffer().content.char(pos);

        // Step 1: If on a non-whitespace char, go to the start of the current WORD.
        if !ch.is_whitespace() {
            while pos > 0 && !self.buffer().content.char(pos - 1).is_whitespace() {
                pos -= 1;
            }
        }

        if pos == 0 {
            let new_line = self.buffer().content.char_to_line(pos);
            let line_start = self.buffer().line_to_char(new_line);
            self.view_mut().cursor.line = new_line;
            self.view_mut().cursor.col = pos - line_start;
            return;
        }

        // Step 2: Move back one char
        pos -= 1;

        // Step 3: Skip whitespace backward
        while pos > 0 && self.buffer().content.char(pos).is_whitespace() {
            pos -= 1;
        }

        if pos == 0 && self.buffer().content.char(pos).is_whitespace() {
            return;
        }

        // pos is now at the last char of the previous WORD
        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    // --- First/last non-blank column helpers ---

    pub(crate) fn first_non_blank_col(&self, line: usize) -> usize {
        if line >= self.buffer().len_lines() {
            return 0;
        }
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);
        for i in 0..line_len {
            let ch = self.buffer().content.char(line_start + i);
            if ch != ' ' && ch != '\t' && ch != '\n' && ch != '\r' {
                return i;
            }
        }
        0
    }

    pub(crate) fn last_non_blank_col(&self, line: usize) -> usize {
        if line >= self.buffer().len_lines() {
            return 0;
        }
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);
        let mut last = 0usize;
        for i in 0..line_len {
            let ch = self.buffer().content.char(line_start + i);
            if ch != '\n' && ch != '\r' && !ch.is_whitespace() {
                last = i;
            }
        }
        last
    }

    // --- Sentence motions ---

    pub(crate) fn move_sentence_forward(&mut self) {
        let total_chars = self.buffer().len_chars();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos >= total_chars {
            return;
        }

        // Advance past current position
        pos += 1;

        // Look for sentence end: '.', '!', or '?' followed by optional closing
        // brackets/quotes, then whitespace
        while pos < total_chars {
            let ch = self.buffer().content.char(pos.saturating_sub(1));
            if matches!(ch, '.' | '!' | '?') {
                // Skip closing brackets/quotes
                while pos < total_chars
                    && matches!(self.buffer().content.char(pos), ')' | ']' | '"' | '\'')
                {
                    pos += 1;
                }
                // Need at least one whitespace after
                if pos < total_chars && self.buffer().content.char(pos).is_whitespace() {
                    // Skip whitespace to land on first char of next sentence
                    while pos < total_chars && self.buffer().content.char(pos).is_whitespace() {
                        pos += 1;
                    }
                    break;
                }
            }
            // Empty line also ends a sentence
            if ch == '\n' && pos < total_chars && self.buffer().content.char(pos) == '\n' {
                pos += 1;
                while pos < total_chars && self.buffer().content.char(pos) == '\n' {
                    pos += 1;
                }
                break;
            }
            pos += 1;
        }

        if pos >= total_chars {
            pos = total_chars.saturating_sub(1);
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    pub(crate) fn move_sentence_backward(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos == 0 {
            return;
        }

        // Step back to skip current whitespace / sentence start
        pos = pos.saturating_sub(1);
        while pos > 0 && self.buffer().content.char(pos).is_whitespace() {
            pos -= 1;
        }

        // Now find the sentence boundary going backward
        while pos > 0 {
            let ch = self.buffer().content.char(pos.saturating_sub(1));
            if matches!(ch, '.' | '!' | '?') {
                // Skip forward past whitespace to land on sentence start
                break;
            }
            // Empty line also signals boundary
            if ch == '\n' && pos > 0 && self.buffer().content.char(pos.saturating_sub(1)) == '\n' {
                break;
            }
            pos -= 1;
        }

        // Skip any leading whitespace at new position
        while pos < self.buffer().len_chars()
            && self.buffer().content.char(pos).is_whitespace()
            && self.buffer().content.char(pos) != '\n'
        {
            pos += 1;
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    // --- Number increment/decrement (Ctrl+a / Ctrl+x) ---

    pub(crate) fn increment_number_at_cursor(&mut self, delta: i64, changed: &mut bool) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);

        let line_text: String = self.buffer().content.line(line).chars().collect();
        let chars: Vec<char> = line_text.chars().collect();

        // Find number at or after cursor
        // First, look for a digit at or after current col
        let mut num_start = None;
        let mut num_end = 0;

        // Check if we're inside a number already
        let search_start = col;
        for i in search_start..chars.len() {
            if chars[i].is_ascii_digit() {
                // Find start of this number (walk back)
                let mut s = i;
                // Check for hex prefix (0x)
                if s > 0 && chars[s - 1] == 'x' && s > 1 && chars[s - 2] == '0' {
                    s -= 2;
                } else if s > 0 && chars[s - 1] == '-' {
                    // Could be negative
                }
                // Find exact start: walk back from i
                let mut start = i;
                while start > 0
                    && (chars[start - 1].is_ascii_digit()
                        || (start >= 2 && chars[start - 1] == 'x' && chars[start - 2] == '0'))
                {
                    start -= 1;
                }
                // Check negative sign
                if start > 0 && chars[start - 1] == '-' {
                    start -= 1;
                }
                // Find end
                let mut end = i;
                while end < chars.len() && chars[end].is_ascii_hexdigit() {
                    end += 1;
                }
                let _ = s;
                num_start = Some(start);
                num_end = end;
                break;
            }
        }

        let start_col = match num_start {
            Some(s) => s,
            None => {
                self.message = "No number under cursor".to_string();
                return;
            }
        };

        let num_str: String = chars[start_col..num_end].iter().collect();
        let trimmed = num_str.trim_start_matches('-');
        let negative = num_str.starts_with('-');

        // Parse the number
        let (value, radix, prefix_len): (i64, u32, usize) =
            if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
                let v = i64::from_str_radix(&trimmed[2..], 16).unwrap_or(0);
                let v = if negative { -v } else { v };
                (v, 16, if negative { 3 } else { 2 })
            } else if trimmed.starts_with('0')
                && trimmed.len() > 1
                && trimmed.chars().all(|c| c.is_ascii_digit())
            {
                let v = i64::from_str_radix(trimmed, 8).unwrap_or(0);
                let v = if negative { -v } else { v };
                (v, 8, 0)
            } else {
                let v: i64 = num_str.parse().unwrap_or(0);
                (v, 10, 0)
            };

        let new_value = value + delta;

        // Format new value preserving radix and padding
        let new_str = if radix == 16 {
            let prefix = if negative { "-0x" } else { "0x" };
            let digits = trimmed.len() - prefix_len; // digit count
            format!(
                "{}{:0>width$x}",
                prefix,
                new_value.unsigned_abs(),
                width = digits
            )
        } else if radix == 8 {
            format!("{:o}", new_value)
        } else {
            format!("{}", new_value)
        };

        // Replace in buffer
        let del_start = line_start + start_col;
        let del_end = line_start + num_end.min(line_len);
        self.start_undo_group();
        self.delete_with_undo(del_start, del_end);
        self.insert_with_undo(del_start, &new_str);
        self.finish_undo_group();

        // Position cursor at end of new number
        self.view_mut().cursor.col = start_col + new_str.chars().count().saturating_sub(1);
        *changed = true;
    }

    // --- Auto-indent lines (= operator) ---

    /// Check whether a line's content (trimmed of trailing newlines) should
    /// trigger an indent increase for the next line.  Language-aware: handles
    /// `{`/`(`/`[` for C-family, `:` for Python, `do`/`then` for Lua/Ruby/Shell.
    fn line_triggers_indent(&self, trimmed: &str) -> bool {
        if trimmed.ends_with('{') || trimmed.ends_with('(') || trimmed.ends_with('[') {
            return true;
        }
        let lang = self
            .buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.file_path.as_ref())
            .and_then(|p| crate::core::lsp::language_id_from_path(p));
        let lang_str = lang.as_deref().unwrap_or("");
        let stripped = trimmed.trim();

        if lang_str == "python" && trimmed.ends_with(':') {
            return true;
        }
        if matches!(lang_str, "lua" | "ruby" | "shellscript" | "bash")
            && (stripped.ends_with(" do")
                || stripped == "do"
                || stripped.ends_with(" then")
                || stripped == "then")
        {
            return true;
        }
        if lang_str == "ruby"
            && (stripped.ends_with(" def")
                || stripped.ends_with(" class")
                || stripped.ends_with(" module")
                || stripped.ends_with(" if")
                || stripped.ends_with(" unless")
                || stripped.ends_with(" begin"))
        {
            return true;
        }
        false
    }

    pub(crate) fn auto_indent_lines(&mut self, line: usize, count: usize, changed: &mut bool) {
        let total_lines = self.buffer().len_lines();
        let end_line = (line + count).min(total_lines);
        if line >= total_lines {
            return;
        }

        let sw = self.effective_shift_width();

        self.start_undo_group();

        for l in line..end_line {
            let cur_indent = self.get_line_indent_str(l);
            // Compute desired indent based on previous non-empty line
            let desired_indent = if l == 0 {
                String::new()
            } else {
                // Find last non-empty line above
                let mut prev = l;
                loop {
                    if prev == 0 {
                        break String::new();
                    }
                    prev -= 1;
                    if !self.is_line_empty(prev) {
                        let prev_indent = self.get_line_indent_str(prev);
                        let prev_text: String = self.buffer().content.line(prev).chars().collect();
                        let prev_trimmed = prev_text.trim_end_matches(['\n', '\r']);
                        if self.line_triggers_indent(prev_trimmed) {
                            let extra = if self.settings.expand_tab {
                                " ".repeat(sw)
                            } else {
                                "\t".to_string()
                            };
                            break format!("{}{}", prev_indent, extra);
                        }
                        // Check if current line starts with '}', ')', ']' — decrease indent
                        let cur_text: String = self.buffer().content.line(l).chars().collect();
                        let cur_trimmed = cur_text.trim_start_matches([' ', '\t']);
                        if cur_trimmed.starts_with('}')
                            || cur_trimmed.starts_with(')')
                            || cur_trimmed.starts_with(']')
                        {
                            let indent_len = prev_indent.len().saturating_sub(sw);
                            break " ".repeat(indent_len);
                        }
                        break prev_indent;
                    }
                }
            };

            if desired_indent != cur_indent {
                let line_start = self.buffer().line_to_char(l);
                let cur_indent_len = cur_indent.chars().count();
                // Remove old indent
                if cur_indent_len > 0 {
                    self.delete_with_undo(line_start, line_start + cur_indent_len);
                }
                // Insert new indent
                if !desired_indent.is_empty() {
                    self.insert_with_undo(line_start, &desired_indent);
                }
            }
        }

        self.finish_undo_group();
        self.clamp_cursor_col();
        *changed = true;
    }

    /// Toggle comments on a range of lines (1-indexed, inclusive).
    ///
    /// Resolves comment style from overrides → built-in table → fallback `#`.
    /// Uses line comments when available, block comments otherwise.
    /// All non-blank lines are toggled: if all are already commented, uncomment;
    /// otherwise add comment markers.
    pub fn toggle_comment(&mut self, start_1: usize, end_1: usize) {
        let buf_id = self.active_buffer_id();
        let lang_id = self
            .buffer_manager
            .get(buf_id)
            .and_then(|s| {
                s.lsp_language_id.clone().or_else(|| {
                    s.file_path
                        .as_ref()
                        .and_then(|p| lsp::language_id_from_path(p))
                })
            })
            .unwrap_or_default();

        let style = comment::resolve_comment_style(&lang_id, &self.comment_overrides);

        let total = self.buffer().len_lines();
        let start = (start_1.saturating_sub(1)).min(total.saturating_sub(1));
        let end = (end_1.saturating_sub(1)).min(total.saturating_sub(1));

        // Collect line texts
        let lines_owned: Vec<String> = (start..=end)
            .map(|i| self.buffer().content.line(i).to_string())
            .collect();
        let lines_ref: Vec<&str> = lines_owned.iter().map(|s| s.as_str()).collect();

        let edits = match comment::compute_toggle_edits(
            &lines_ref,
            &style.line,
            &style.block_open,
            &style.block_close,
        ) {
            Some(e) => e,
            None => return,
        };

        self.start_undo_group();
        // Apply edits in reverse order so char offsets remain valid
        for edit in edits.iter().rev() {
            let line_idx = start + edit.line_idx;
            let line_start = self.buffer().line_to_char(line_idx);
            let line_end = if line_idx + 1 < self.buffer().len_lines() {
                self.buffer().line_to_char(line_idx + 1)
            } else {
                self.buffer().len_chars()
            };
            let new_line = format!("{}\n", edit.new_text);
            self.delete_with_undo(line_start, line_end);
            self.insert_with_undo(line_start, &new_line);
        }
        self.finish_undo_group();
        self.set_dirty(true);
    }

    /// Populate `comment_overrides` from installed extension manifests.
    /// Called once at plugin init time; manifest `[comment]` sections with
    /// non-empty `line` or `block_open` are applied for each `language_id`.
    pub(crate) fn populate_comment_overrides(&mut self) {
        for manifest in self.ext_available_manifests() {
            if !self.extension_state.is_installed(&manifest.name) {
                continue;
            }
            if let Some(cc) = &manifest.comment {
                if cc.line.is_empty() && cc.block_open.is_empty() {
                    continue;
                }
                let style = comment::CommentStyleOwned {
                    line: cc.line.clone(),
                    block_open: cc.block_open.clone(),
                    block_close: cc.block_close.clone(),
                };
                for lang_id in &manifest.language_ids {
                    // Don't overwrite runtime (plugin) overrides
                    self.comment_overrides
                        .entry(lang_id.clone())
                        .or_insert_with(|| style.clone());
                }
            }
        }
    }

    /// Populate highlight query overrides from installed extension manifests,
    /// then re-apply to any already-open buffers whose language matches.
    pub(crate) fn populate_highlight_overrides(&mut self) {
        for manifest in self.ext_available_manifests() {
            if !self.extension_state.is_installed(&manifest.name) {
                continue;
            }
            if let Some(ref hl) = manifest.highlights {
                if hl.is_empty() {
                    continue;
                }
                for lang_id in &manifest.language_ids {
                    self.highlight_overrides
                        .entry(lang_id.clone())
                        .or_insert_with(|| hl.clone());
                }
            }
        }
        // Re-apply to open buffers so files opened before extensions loaded
        // pick up the override queries.
        if !self.highlight_overrides.is_empty() {
            let ids: Vec<_> = self.buffer_manager.list().to_vec();
            for bid in ids {
                if let Some(state) = self.buffer_manager.get_mut(bid) {
                    if let Some(ref path) = state.file_path.clone() {
                        if let Some(syn) = crate::core::syntax::Syntax::new_from_path_with_overrides(
                            path.to_str(),
                            Some(&self.highlight_overrides),
                        ) {
                            state.syntax = Some(syn);
                            state.update_syntax();
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn format_lines(&mut self, start_line: usize, end_line: usize, changed: &mut bool) {
        let total = self.buffer().len_lines();
        let start = start_line.min(total.saturating_sub(1));
        let end = end_line.min(total.saturating_sub(1));
        let tw = if self.settings.textwidth > 0 {
            self.settings.textwidth
        } else {
            79
        };

        // Collect the text of the range
        let mut text = String::new();
        for l in start..=end {
            let line: String = self.buffer().content.line(l).chars().collect();
            let trimmed = line.trim_end_matches(['\n', '\r']);
            if trimmed.is_empty() {
                // Paragraph break — preserve it
                text.push('\n');
            } else {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push(' ');
                }
                text.push_str(trimmed);
            }
        }

        // Reflow: split into paragraphs, wrap each
        let paragraphs: Vec<&str> = text.split('\n').collect();
        let mut result = String::new();
        for (pi, para) in paragraphs.iter().enumerate() {
            if para.is_empty() {
                result.push('\n');
                continue;
            }
            let words: Vec<&str> = para.split_whitespace().collect();
            let mut line_buf = String::new();
            for word in &words {
                if line_buf.is_empty() {
                    line_buf.push_str(word);
                } else if line_buf.len() + 1 + word.len() > tw {
                    result.push_str(&line_buf);
                    result.push('\n');
                    line_buf = word.to_string();
                } else {
                    line_buf.push(' ');
                    line_buf.push_str(word);
                }
            }
            if !line_buf.is_empty() {
                result.push_str(&line_buf);
                if pi < paragraphs.len() - 1 || end + 1 < total {
                    result.push('\n');
                }
            }
        }

        // Replace the range
        let range_start = self.buffer().line_to_char(start);
        let range_end = if end + 1 < total {
            self.buffer().line_to_char(end + 1)
        } else {
            self.buffer().len_chars()
        };

        self.start_undo_group();
        self.delete_with_undo(range_start, range_end);
        self.insert_with_undo(range_start, &result);
        self.finish_undo_group();

        // Move cursor to start of formatted area
        self.view_mut().cursor.line = start;
        self.view_mut().cursor.col = self.first_non_blank_col(start);
        *changed = true;
    }

    // --- WORD text object (iW / aW) ---

    pub(crate) fn find_bigword_object(
        &self,
        modifier: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        let total_chars = self.buffer().len_chars();
        if cursor_pos >= total_chars {
            return None;
        }

        let char_at_cursor = self.buffer().content.char(cursor_pos);

        // If on whitespace and modifier is 'i', no match
        if modifier == 'i' && char_at_cursor.is_whitespace() {
            return None;
        }

        let mut start = cursor_pos;
        let mut end = cursor_pos;

        // Expand backward to start of WORD (non-whitespace)
        while start > 0 && !self.buffer().content.char(start - 1).is_whitespace() {
            start -= 1;
        }

        // Expand forward to end of WORD
        while end < total_chars && !self.buffer().content.char(end).is_whitespace() {
            end += 1;
        }

        // For 'aW', include trailing whitespace
        if modifier == 'a' {
            while end < total_chars {
                let ch = self.buffer().content.char(end);
                if !ch.is_whitespace() || ch == '\n' {
                    break;
                }
                end += 1;
            }
        }

        if start < end {
            Some((start, end))
        } else {
            None
        }
    }

    // --- gJ: join lines without inserting space ---

    pub(crate) fn join_lines_no_space(&mut self, count: usize, changed: &mut bool) {
        let total_lines = self.buffer().len_lines();
        let start_line = self.view().cursor.line;
        let joins = count.min(total_lines.saturating_sub(start_line + 1));
        if joins == 0 {
            return;
        }

        self.start_undo_group();
        for _ in 0..joins {
            let cur_line = self.view().cursor.line;
            let next_line = cur_line + 1;
            if next_line >= self.buffer().len_lines() {
                break;
            }

            let cur_line_len = self.buffer().line_len_chars(cur_line);
            let cur_line_start = self.buffer().line_to_char(cur_line);
            let newline_pos = cur_line_start + cur_line_len - 1;

            // Count leading whitespace on next line
            let next_line_content: String = self.buffer().content.line(next_line).chars().collect();
            let leading_ws = next_line_content
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .count();

            // Delete newline + all leading whitespace of next line (no space inserted)
            let next_line_start = self.buffer().line_to_char(next_line);
            let del_end = next_line_start + leading_ws;
            self.delete_with_undo(newline_pos, del_end);
        }
        self.finish_undo_group();

        self.clamp_cursor_col();
        *changed = true;
    }

    // --- gf: open file path under cursor ---

    pub(crate) fn file_path_under_cursor(&self) -> Option<std::path::PathBuf> {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let total_chars = self.buffer().len_chars();
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);

        let line_text: String = self.buffer().content.line(line).chars().collect();
        let chars: Vec<char> = line_text.chars().collect();

        // Find boundaries of path-like token at cursor (non-whitespace, non-quote chars)
        let is_path_char = |c: char| {
            !c.is_whitespace() && c != '"' && c != '\'' && c != ':' && c != ',' && c != ';'
        };

        let _ = total_chars;
        let _ = line_len;

        let mut start = col;
        let mut end = col;

        while start > 0 && is_path_char(chars[start - 1]) {
            start -= 1;
        }
        while end < chars.len() && is_path_char(chars[end]) {
            end += 1;
        }
        // Strip trailing newline chars
        while end > start && (chars[end - 1] == '\n' || chars[end - 1] == '\r') {
            end -= 1;
        }

        if start >= end {
            return None;
        }

        let _ = line_start;
        let path_str: String = chars[start..end].iter().collect();
        if path_str.is_empty() {
            return None;
        }

        let path = std::path::PathBuf::from(&path_str);

        // Try relative to workspace root, then to current file's dir
        if path.is_absolute() {
            if path.exists() {
                return Some(path);
            }
        } else {
            if let Some(ref root) = self.workspace_root {
                let abs = root.join(&path);
                if abs.exists() {
                    return Some(abs);
                }
            }
            if let Some(file_path) = self.active_buffer_state().file_path.as_ref() {
                if let Some(dir) = file_path.parent() {
                    let abs = dir.join(&path);
                    if abs.exists() {
                        return Some(abs);
                    }
                }
            }
        }

        None
    }

    // --- g* / g#: partial word search ---

    pub(crate) fn search_word_under_cursor_partial(&mut self, forward: bool) {
        let word = match self.word_under_cursor() {
            Some(w) => w,
            None => {
                self.message = "No word under cursor".to_string();
                return;
            }
        };

        self.search_query = word.clone();
        self.search_direction = if forward {
            SearchDirection::Forward
        } else {
            SearchDirection::Backward
        };
        self.search_word_bounded = false;

        // Use plain text search (no word boundaries)
        self.run_search();

        if self.search_matches.is_empty() {
            self.message = format!("Pattern not found: {}", word);
            return;
        }

        if forward {
            self.search_next();
        } else {
            self.search_prev();
        }
    }

    // --- ]p / [p: paste with indent adjustment ---

    pub(crate) fn paste_after_adjusted_indent(&mut self, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register_content(reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                return;
            }
        };

        if !is_linewise {
            // For characterwise, just paste normally
            self.paste_after(changed);
            return;
        }

        let cur_line = self.view().cursor.line;
        let cur_indent = self.get_line_indent_str(cur_line);
        let sw = self.effective_shift_width();

        // Adjust each pasted line's indent to match current line
        let adjusted = self.adjust_paste_indent(&content, &cur_indent, sw);

        self.start_undo_group();
        let line_end =
            self.buffer().line_to_char(cur_line) + self.buffer().line_len_chars(cur_line);
        let last_char = if self.buffer().line_len_chars(cur_line) > 0 {
            self.buffer().content.char(line_end - 1)
        } else {
            '\0'
        };
        if last_char == '\n' {
            self.insert_with_undo(line_end, &adjusted);
        } else {
            let s = format!("\n{}", adjusted);
            self.insert_with_undo(line_end, &s);
        }
        self.view_mut().cursor.line += 1;
        self.view_mut().cursor.col = 0;
        self.finish_undo_group();
        self.clear_selected_register();
        *changed = true;
    }

    pub(crate) fn paste_before_adjusted_indent(&mut self, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register_content(reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                return;
            }
        };

        if !is_linewise {
            self.paste_before(changed);
            return;
        }

        let cur_line = self.view().cursor.line;
        let cur_indent = self.get_line_indent_str(cur_line);
        let sw = self.effective_shift_width();

        let adjusted = self.adjust_paste_indent(&content, &cur_indent, sw);

        self.start_undo_group();
        let line_start = self.buffer().line_to_char(cur_line);
        self.insert_with_undo(line_start, &adjusted);
        self.view_mut().cursor.col = 0;
        self.finish_undo_group();
        self.clear_selected_register();
        *changed = true;
    }

    /// Adjust each line's indentation in `text` to match `target_indent`.
    pub(crate) fn adjust_paste_indent(&self, text: &str, target_indent: &str, sw: usize) -> String {
        let _ = sw;
        let lines: Vec<&str> = text.lines().collect();
        if lines.is_empty() {
            return text.to_string();
        }

        // Determine the minimum indent of pasted content
        let min_indent = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);

        let mut result = String::new();
        for (i, l) in lines.iter().enumerate() {
            let cur_indent_len = l.len() - l.trim_start().len();
            let excess = cur_indent_len.saturating_sub(min_indent);
            let extra = " ".repeat(excess);
            let new_line = format!("{}{}{}", target_indent, extra, l.trim_start());
            result.push_str(&new_line);
            if i + 1 < lines.len() || text.ends_with('\n') {
                result.push('\n');
            }
        }
        result
    }

    // --- Replace mode key handler ---

    pub(crate) fn handle_replace_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
        changed: &mut bool,
    ) {
        let _ = ctrl;
        match key_name {
            "Escape" => {
                self.virtual_replace = false;
                self.mode = Mode::Normal;
                self.clamp_cursor_col();
            }
            "BackSpace" => {
                // In replace mode, backspace just moves cursor back (simplified)
                self.move_left();
            }
            "Left" => self.move_left(),
            "Right" => self.move_right(),
            "Up" => {
                if self.view().cursor.line > 0 {
                    self.view_mut().cursor.line -= 1;
                    self.clamp_cursor_col();
                }
            }
            "Down" => {
                let max_line = self.buffer().len_lines().saturating_sub(1);
                if self.view().cursor.line < max_line {
                    self.view_mut().cursor.line += 1;
                    self.clamp_cursor_col();
                }
            }
            _ => {
                if let Some(ch) = unicode {
                    let line = self.view().cursor.line;
                    let col = self.view().cursor.col;
                    let line_len = self.buffer().line_len_chars(line);
                    let char_idx = self.buffer().line_to_char(line) + col;

                    // At or beyond end of line: just insert (like insert mode)
                    let line_content_len = if line_len > 0 {
                        let last = self
                            .buffer()
                            .content
                            .char(self.buffer().line_to_char(line) + line_len - 1);
                        if last == '\n' {
                            line_len - 1
                        } else {
                            line_len
                        }
                    } else {
                        0
                    };

                    // Virtual Replace: expand tab to spaces before overwriting
                    if self.virtual_replace && col < line_content_len {
                        let cur_char = self.buffer().content.char(char_idx);
                        if cur_char == '\t' {
                            let tabstop = self.settings.tabstop as usize;
                            // Calculate visual column of cursor
                            let line_start = self.buffer().line_to_char(line);
                            let mut vcol = 0usize;
                            for i in 0..col {
                                let c = self.buffer().content.char(line_start + i);
                                if c == '\t' {
                                    vcol = (vcol / tabstop + 1) * tabstop;
                                } else {
                                    vcol += 1;
                                }
                            }
                            let tab_width = tabstop - (vcol % tabstop);
                            // Replace tab with spaces, then overwrite first space
                            self.start_undo_group();
                            self.delete_with_undo(char_idx, char_idx + 1);
                            let spaces = " ".repeat(tab_width);
                            self.insert_with_undo(char_idx, &spaces);
                            // Now overwrite the first space with the typed char
                            self.delete_with_undo(char_idx, char_idx + 1);
                            let mut buf = [0u8; 4];
                            let s = ch.encode_utf8(&mut buf);
                            self.insert_with_undo(char_idx, s);
                            self.view_mut().cursor.col += 1;
                            self.finish_undo_group();
                            *changed = true;
                            return;
                        }
                    }

                    self.start_undo_group();
                    if col < line_content_len {
                        // Overwrite: delete one char, insert replacement
                        self.delete_with_undo(char_idx, char_idx + 1);
                        let mut buf = [0u8; 4];
                        let s = ch.encode_utf8(&mut buf);
                        self.insert_with_undo(char_idx, s);
                        self.view_mut().cursor.col += 1;
                    } else {
                        // Past end of line: insert
                        let mut buf = [0u8; 4];
                        let s = ch.encode_utf8(&mut buf);
                        self.insert_with_undo(char_idx, s);
                        self.view_mut().cursor.col += 1;
                    }
                    self.finish_undo_group();
                    *changed = true;
                }
            }
        }
    }

    // --- Paragraph motions ---

    pub(crate) fn move_paragraph_forward(&mut self) {
        let total_lines = self.buffer().len_lines();
        let mut line = self.view().cursor.line;

        // Move forward at least one line to find the next empty line
        if line + 1 >= total_lines {
            // Already at or past last line, don't move
            return;
        }
        line += 1;

        // Search for the next empty line
        while line < total_lines && !self.is_line_empty(line) {
            line += 1;
        }

        // Move to the empty line we found, or the last line if we hit EOF
        if line >= total_lines {
            line = total_lines.saturating_sub(1);
        }
        self.view_mut().cursor.line = line;
        self.view_mut().cursor.col = self.get_line_len_for_insert(line);
    }

    pub(crate) fn move_paragraph_backward(&mut self) {
        let mut line = self.view().cursor.line;

        // Already at top, don't move
        if line == 0 {
            return;
        }
        line -= 1;

        // Search backward for an empty line
        while line > 0 && !self.is_line_empty(line) {
            line -= 1;
        }

        // Move to the found empty line (or line 0 if that's where we stopped)
        self.view_mut().cursor.line = line;
        // Move to end of line (column 0 for empty lines)
        self.view_mut().cursor.col = self.get_line_len_for_insert(line);
    }

    /// Returns true if the line is empty or contains only whitespace.
    pub(crate) fn is_line_empty(&self, line: usize) -> bool {
        if line >= self.buffer().len_lines() {
            return false;
        }

        let line_len = self.buffer().line_len_chars(line);

        // Line with no characters or just newline
        if line_len == 0 || line_len == 1 {
            return true;
        }

        // Check if all characters are whitespace
        let line_start = self.buffer().line_to_char(line);
        for i in 0..line_len {
            let ch = self.buffer().content.char(line_start + i);
            if ch != '\n' && !ch.is_whitespace() {
                return false;
            }
        }

        true
    }

    // --- Character find motions (f, F, t, T, ;, ,) ---

    /// Find a character on the current line.
    /// motion_type: 'f' (forward inclusive), 'F' (backward inclusive),
    ///              't' (forward till/exclusive), 'T' (backward till/exclusive)
    pub(crate) fn find_char(&mut self, motion_type: char, target: char) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);

        match motion_type {
            'f' => {
                // Find forward (inclusive): search right of cursor
                for i in (col + 1)..line_len {
                    let ch = self.buffer().content.char(line_start + i);
                    if ch == target && ch != '\n' {
                        self.view_mut().cursor.col = i;
                        return;
                    }
                }
            }
            'F' => {
                // Find backward (inclusive): search left of cursor
                if col > 0 {
                    for i in (0..col).rev() {
                        let ch = self.buffer().content.char(line_start + i);
                        if ch == target {
                            self.view_mut().cursor.col = i;
                            return;
                        }
                    }
                }
            }
            't' => {
                // Till forward (exclusive): stop before target
                for i in (col + 1)..line_len {
                    let ch = self.buffer().content.char(line_start + i);
                    if ch == target && ch != '\n' {
                        if i > 0 {
                            self.view_mut().cursor.col = i - 1;
                        }
                        return;
                    }
                }
            }
            'T' => {
                // Till backward (exclusive): stop after target
                if col > 0 {
                    for i in (0..col).rev() {
                        let ch = self.buffer().content.char(line_start + i);
                        if ch == target {
                            self.view_mut().cursor.col = i + 1;
                            return;
                        }
                    }
                }
            }
            _ => {}
        }
        // Character not found - cursor doesn't move (Vim behavior)
    }

    /// Repeat the last character find motion.
    /// If reverse is true, search in the opposite direction.
    pub(crate) fn repeat_find(&mut self, reverse: bool) {
        if let Some((motion_type, target)) = self.last_find {
            let actual_motion = if reverse {
                // Reverse the direction
                match motion_type {
                    'f' => 'F',
                    'F' => 'f',
                    't' => 'T',
                    'T' => 't',
                    _ => motion_type,
                }
            } else {
                motion_type
            };
            self.find_char(actual_motion, target);
        }
    }

    // --- Bracket matching (%) ---

    pub(crate) fn move_to_matching_bracket(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let char_pos = self.buffer().line_to_char(line) + col;

        if char_pos >= self.buffer().len_chars() {
            return;
        }

        let current_char = self.buffer().content.char(char_pos);

        // Check if current character is a bracket and determine search parameters
        let (is_opening, open_char, close_char) = match current_char {
            '(' => (true, '(', ')'),
            ')' => (false, '(', ')'),
            '{' => (true, '{', '}'),
            '}' => (false, '{', '}'),
            '[' => (true, '[', ']'),
            ']' => (false, '[', ']'),
            _ => {
                // Not on a bracket, search forward on current line for next bracket
                self.search_forward_for_bracket();
                return;
            }
        };

        // Find matching bracket
        if let Some(match_pos) =
            self.find_matching_bracket(char_pos, open_char, close_char, is_opening)
        {
            let new_line = self.buffer().content.char_to_line(match_pos);
            let line_start = self.buffer().line_to_char(new_line);
            self.view_mut().cursor.line = new_line;
            self.view_mut().cursor.col = match_pos - line_start;
        }
    }

    pub(crate) fn search_forward_for_bracket(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);

        // Search forward from cursor position for any bracket
        for i in col..line_len {
            let pos = line_start + i;
            if pos >= self.buffer().len_chars() {
                return;
            }
            let ch = self.buffer().content.char(pos);
            match ch {
                '(' | ')' | '{' | '}' | '[' | ']' => {
                    self.view_mut().cursor.col = i;
                    // Now move to matching bracket
                    self.move_to_matching_bracket();
                    return;
                }
                '\n' => return, // Don't go past end of line
                _ => {}
            }
        }
    }

    pub(crate) fn find_matching_bracket(
        &self,
        start_pos: usize,
        open_char: char,
        close_char: char,
        is_opening: bool,
    ) -> Option<usize> {
        let total_chars = self.buffer().len_chars();
        let mut depth = 1;

        if is_opening {
            // Search forward
            let mut pos = start_pos + 1;
            while pos < total_chars {
                let ch = self.buffer().content.char(pos);
                if ch == open_char {
                    depth += 1;
                } else if ch == close_char {
                    depth -= 1;
                    if depth == 0 {
                        return Some(pos);
                    }
                }
                pos += 1;
            }
        } else {
            // Search backward
            if start_pos == 0 {
                return None;
            }
            let mut pos = start_pos - 1;
            loop {
                let ch = self.buffer().content.char(pos);
                if ch == open_char {
                    depth -= 1;
                    if depth == 0 {
                        return Some(pos);
                    }
                } else if ch == close_char {
                    depth += 1;
                }
                if pos == 0 {
                    break;
                }
                pos -= 1;
            }
        }

        None
    }

    /// Update `self.bracket_match` based on the character under the cursor.
    /// Called at the end of `handle_key()` when `match_brackets` is enabled.
    pub fn update_bracket_match(&mut self) {
        if !self.settings.match_brackets {
            self.bracket_match = None;
            return;
        }
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_start = self.buffer().line_to_char(line);
        let char_pos = line_start + col;
        if char_pos >= self.buffer().len_chars() {
            self.bracket_match = None;
            return;
        }
        let current_char = self.buffer().content.char(char_pos);
        let (is_opening, open_char, close_char) = match current_char {
            '(' => (true, '(', ')'),
            ')' => (false, '(', ')'),
            '{' => (true, '{', '}'),
            '}' => (false, '{', '}'),
            '[' => (true, '[', ']'),
            ']' => (false, '[', ']'),
            _ => {
                self.bracket_match = None;
                return;
            }
        };
        if let Some(match_pos) =
            self.find_matching_bracket(char_pos, open_char, close_char, is_opening)
        {
            let match_line = self.buffer().content.char_to_line(match_pos);
            let match_line_start = self.buffer().line_to_char(match_line);
            self.bracket_match = Some((match_line, match_pos - match_line_start));
        } else {
            self.bracket_match = None;
        }
    }

    /// Find the range for a text object.
    /// Returns (start_pos, end_pos) if found, None otherwise.
    pub(crate) fn find_text_object_range(
        &self,
        modifier: char,
        obj_type: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        match obj_type {
            'w' => self.find_word_object(modifier, cursor_pos),
            'W' => self.find_bigword_object(modifier, cursor_pos),
            '"' => self.find_quote_object(modifier, '"', cursor_pos),
            '\'' => self.find_quote_object(modifier, '\'', cursor_pos),
            '(' | ')' => self.find_bracket_object(modifier, '(', ')', cursor_pos),
            '{' | '}' => self.find_bracket_object(modifier, '{', '}', cursor_pos),
            '[' | ']' => self.find_bracket_object(modifier, '[', ']', cursor_pos),
            '<' | '>' => self.find_bracket_object(modifier, '<', '>', cursor_pos),
            'p' => self.find_paragraph_object(modifier, cursor_pos),
            's' => self.find_sentence_object(modifier, cursor_pos),
            't' => self.find_tag_text_object(modifier, cursor_pos),
            '`' => self.find_quote_object(modifier, '`', cursor_pos),
            'e' => self.find_latex_environment_object(modifier, cursor_pos),
            'c' if self.is_latex_buffer() => self.find_latex_command_object(modifier, cursor_pos),
            '$' => self.find_latex_math_object(modifier, cursor_pos),
            _ => None,
        }
    }

    /// Find word text object range (iw/aw)
    pub(crate) fn find_word_object(
        &self,
        modifier: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        let total_chars = self.buffer().len_chars();
        if cursor_pos >= total_chars {
            return None;
        }

        let char_at_cursor = self.buffer().content.char(cursor_pos);

        // If on whitespace and modifier is 'i', no match
        if modifier == 'i' && (char_at_cursor.is_whitespace() && char_at_cursor != '\n') {
            return None;
        }

        // Find word boundaries
        let mut start = cursor_pos;
        let mut end = cursor_pos;

        // Expand backward to start of word
        while start > 0 {
            let ch = self.buffer().content.char(start - 1);
            if ch.is_whitespace() || !is_word_char(ch) {
                break;
            }
            start -= 1;
        }

        // Expand forward to end of word
        while end < total_chars {
            let ch = self.buffer().content.char(end);
            if ch.is_whitespace() || !is_word_char(ch) {
                break;
            }
            end += 1;
        }

        // For 'aw', include trailing whitespace
        if modifier == 'a' {
            while end < total_chars {
                let ch = self.buffer().content.char(end);
                if !ch.is_whitespace() || ch == '\n' {
                    break;
                }
                end += 1;
            }
        }

        if start < end {
            Some((start, end))
        } else {
            None
        }
    }

    /// Find quote text object range (i"/a")
    pub(crate) fn find_quote_object(
        &self,
        modifier: char,
        quote_char: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        let total_chars = self.buffer().len_chars();
        if cursor_pos >= total_chars {
            return None;
        }

        // Get current line bounds to search within
        let cursor_line = self.buffer().content.char_to_line(cursor_pos);
        let line_start = self.buffer().line_to_char(cursor_line);
        let line_len = self.buffer().line_len_chars(cursor_line);
        let line_end = line_start + line_len;

        // Find opening quote (search backward from cursor)
        let mut open_pos = None;
        let mut pos = cursor_pos;
        while pos >= line_start {
            let ch = self.buffer().content.char(pos);
            if ch == quote_char {
                // Check if it's escaped
                if pos == line_start || self.buffer().content.char(pos - 1) != '\\' {
                    open_pos = Some(pos);
                    break;
                }
            }
            if pos == line_start {
                break;
            }
            pos -= 1;
        }

        let open_pos = open_pos?;

        // Find closing quote (search forward from opening)
        let mut close_pos = None;
        let mut pos = open_pos + 1;
        while pos < line_end {
            let ch = self.buffer().content.char(pos);
            if ch == quote_char {
                // Check if it's escaped
                if self.buffer().content.char(pos - 1) != '\\' {
                    close_pos = Some(pos);
                    break;
                }
            }
            pos += 1;
        }

        let close_pos = close_pos?;

        // Return range based on modifier
        if modifier == 'i' {
            // Inner: exclude quotes
            if open_pos < close_pos {
                Some((open_pos + 1, close_pos))
            } else {
                None
            }
        } else {
            // Around: include quotes + trailing whitespace (or leading if no trailing)
            let mut end = close_pos + 1;
            let mut start = open_pos;
            // Try trailing whitespace first
            let mut trail = end;
            while trail < line_end {
                let ch = self.buffer().content.char(trail);
                if ch == ' ' || ch == '\t' {
                    trail += 1;
                } else {
                    break;
                }
            }
            if trail > end {
                end = trail;
            } else {
                // No trailing whitespace — try leading whitespace
                let mut lead = start;
                while lead > line_start {
                    let ch = self.buffer().content.char(lead - 1);
                    if ch == ' ' || ch == '\t' {
                        lead -= 1;
                    } else {
                        break;
                    }
                }
                start = lead;
            }
            Some((start, end))
        }
    }

    /// Find bracket text object range (i(/a()
    pub(crate) fn find_bracket_object(
        &self,
        modifier: char,
        open_char: char,
        close_char: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        let total_chars = self.buffer().len_chars();
        if cursor_pos >= total_chars {
            return None;
        }

        // Find the nearest enclosing bracket pair
        let mut open_pos = None;
        let mut depth = 0;

        // Search backward for opening bracket
        let mut pos = cursor_pos;
        loop {
            let ch = self.buffer().content.char(pos);
            if ch == close_char {
                depth += 1;
            } else if ch == open_char {
                if depth == 0 {
                    open_pos = Some(pos);
                    break;
                } else {
                    depth -= 1;
                }
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
        }

        let open_pos = open_pos?;

        // Find matching closing bracket
        let close_pos = self.find_matching_bracket(open_pos, open_char, close_char, true)?;

        // Return range based on modifier
        if modifier == 'i' {
            // Inner: exclude brackets
            if open_pos < close_pos {
                Some((open_pos + 1, close_pos))
            } else {
                None
            }
        } else {
            // Around: include brackets
            Some((open_pos, close_pos + 1))
        }
    }

    /// Find paragraph text object range (ip/ap).
    ///
    /// A paragraph is a contiguous block of lines that are all blank or all non-blank.
    /// `ip` (inner) selects those lines; `ap` (around) also includes any trailing blank lines
    /// (or leading ones when the paragraph is at the end of the buffer).
    pub(crate) fn find_paragraph_object(
        &self,
        modifier: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        let total_lines = self.buffer().len_lines();
        if total_lines == 0 {
            return None;
        }

        let safe_pos = cursor_pos.min(self.buffer().len_chars().saturating_sub(1));
        let cursor_line = self.buffer().content.char_to_line(safe_pos);
        let on_blank = self.is_line_empty(cursor_line);

        // Extend upward while lines share the same blank/non-blank type.
        let mut start_line = cursor_line;
        while start_line > 0 && self.is_line_empty(start_line - 1) == on_blank {
            start_line -= 1;
        }

        // Extend downward while lines share the same blank/non-blank type.
        let mut end_line = cursor_line;
        while end_line + 1 < total_lines && self.is_line_empty(end_line + 1) == on_blank {
            end_line += 1;
        }

        // `ap` on a non-blank paragraph: include the following blank lines.
        // If there are no following blank lines (end of file), include any preceding ones.
        if modifier == 'a' && !on_blank {
            if end_line + 1 < total_lines && self.is_line_empty(end_line + 1) {
                while end_line + 1 < total_lines && self.is_line_empty(end_line + 1) {
                    end_line += 1;
                }
            } else if start_line > 0 && self.is_line_empty(start_line - 1) {
                while start_line > 0 && self.is_line_empty(start_line - 1) {
                    start_line -= 1;
                }
            }
        }

        let start_pos = self.buffer().line_to_char(start_line);
        let end_pos = if end_line + 1 < total_lines {
            self.buffer().line_to_char(end_line + 1)
        } else {
            self.buffer().len_chars()
        };

        if start_pos < end_pos {
            Some((start_pos, end_pos))
        } else {
            None
        }
    }

    /// Find sentence text object range (is/as).
    ///
    /// A sentence ends at `.`, `!`, or `?` followed by whitespace or end-of-buffer.
    /// A blank line also terminates a sentence (paragraph boundary).
    /// `is` (inner) selects the sentence text without leading whitespace.
    /// `as` (around) additionally includes the trailing whitespace after the punctuation.
    pub(crate) fn find_sentence_object(
        &self,
        modifier: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        let total_chars = self.buffer().len_chars();
        if total_chars == 0 || cursor_pos >= total_chars {
            return None;
        }

        // Returns true if the character at `pos` is sentence-ending punctuation AND
        // it is followed by whitespace (or is at the end of the buffer).
        let is_sentence_end_punct = |pos: usize| -> bool {
            if pos >= total_chars {
                return false;
            }
            let ch = self.buffer().content.char(pos);
            if !matches!(ch, '.' | '!' | '?') {
                return false;
            }
            pos + 1 >= total_chars || self.buffer().content.char(pos + 1).is_whitespace()
        };

        // Returns true if `pos` is the start of a blank line (the \n of a blank line).
        let is_blank_line = |pos: usize| -> bool {
            if pos >= total_chars {
                return false;
            }
            let ch = self.buffer().content.char(pos);
            ch == '\n' && (pos == 0 || self.buffer().content.char(pos.saturating_sub(1)) == '\n')
        };

        // --- Find start of current sentence (scan backward) ---
        let mut sent_start = 0usize;
        if cursor_pos > 0 {
            let mut pos = cursor_pos - 1;
            loop {
                if is_sentence_end_punct(pos) {
                    sent_start = pos + 1;
                    break;
                }
                if is_blank_line(pos) {
                    // Paragraph boundary — sentence starts right after this \n.
                    sent_start = pos + 1;
                    break;
                }
                if pos == 0 {
                    sent_start = 0;
                    break;
                }
                pos -= 1;
            }
        }

        // --- Find end of current sentence (scan forward) ---
        let mut sent_end = total_chars; // default: end of buffer
        let mut pos = cursor_pos;
        while pos < total_chars {
            if is_sentence_end_punct(pos) {
                sent_end = pos + 1; // include the punctuation
                break;
            }
            // Blank line ends the sentence too.
            if self.buffer().content.char(pos) == '\n'
                && pos + 1 < total_chars
                && self.buffer().content.char(pos + 1) == '\n'
            {
                sent_end = pos + 1; // include up to the blank-line newline
                break;
            }
            pos += 1;
        }

        // Skip leading whitespace for the inner start.
        let mut inner_start = sent_start;
        while inner_start < sent_end {
            let ch = self.buffer().content.char(inner_start);
            if !ch.is_whitespace() {
                break;
            }
            inner_start += 1;
        }

        let (start, end) = if modifier == 'i' {
            (inner_start, sent_end)
        } else {
            // `as`: include trailing whitespace (spaces/tabs only, not newlines).
            let mut e = sent_end;
            while e < total_chars {
                let ch = self.buffer().content.char(e);
                if ch == '\n' || !ch.is_whitespace() {
                    break;
                }
                e += 1;
            }
            (inner_start, e)
        };

        if start < end {
            Some((start, end))
        } else {
            None
        }
    }

    /// Find tag text object range (it/at).
    ///
    /// `it` (inner tag) selects the content between the nearest enclosing open and close tag.
    /// `at` (around tag) includes the opening and closing tags themselves.
    /// Tag-name comparison is case-insensitive; nested same-name tags are handled by
    /// depth tracking during the forward scan for the closing tag.
    pub(crate) fn find_tag_text_object(
        &self,
        modifier: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        let total_chars = self.buffer().len_chars();
        if total_chars == 0 || cursor_pos >= total_chars {
            return None;
        }

        // Safe single-character accessor.
        let ch = |pos: usize| -> char {
            if pos < total_chars {
                self.buffer().content.char(pos)
            } else {
                '\0'
            }
        };

        // Try to parse an HTML/XML tag beginning at `start` (which must hold '<').
        // Returns (tag_name_lowercase, is_closing, is_self_closing, pos_after_close_angle).
        // Returns None for comments (<!--), processing instructions (<?), doctypes (<!),
        // or malformed tags.
        let parse_tag_at = |start: usize| -> Option<(String, bool, bool, usize)> {
            if ch(start) != '<' {
                return None;
            }
            let mut pos = start + 1;
            if pos >= total_chars {
                return None;
            }
            let c1 = ch(pos);
            // Skip comments (<!), doctype (<!), processing instructions (<?)
            if c1 == '!' || c1 == '?' {
                return None;
            }
            let is_closing = c1 == '/';
            if is_closing {
                pos += 1;
            }
            // Tag name must start with an ASCII letter or underscore.
            if !ch(pos).is_ascii_alphabetic() && ch(pos) != '_' {
                return None;
            }
            let name_start = pos;
            while pos < total_chars {
                let c = ch(pos);
                if c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.') {
                    pos += 1;
                } else {
                    break;
                }
            }
            let tag_name: String = (name_start..pos)
                .map(&ch)
                .collect::<String>()
                .to_ascii_lowercase();
            if tag_name.is_empty() {
                return None;
            }
            // Scan forward to the closing '>', handling quoted attribute values.
            let mut in_quote: Option<char> = None;
            let mut is_self_closing = false;
            while pos < total_chars {
                let c = ch(pos);
                match in_quote {
                    Some(q) => {
                        if c == q {
                            in_quote = None;
                        }
                    }
                    None => match c {
                        '"' | '\'' => {
                            in_quote = Some(c);
                        }
                        '/' if ch(pos + 1) == '>' => {
                            is_self_closing = true;
                        }
                        '>' => {
                            return Some((tag_name, is_closing, is_self_closing, pos + 1));
                        }
                        _ => {}
                    },
                }
                pos += 1;
            }
            None // unclosed tag
        };

        // Main loop: walk backward from cursor_pos looking for an enclosing open tag.
        let mut scan_pos = cursor_pos;
        loop {
            // Walk backward to the nearest '<'.
            while ch(scan_pos) != '<' {
                if scan_pos == 0 {
                    return None;
                }
                scan_pos -= 1;
            }
            let open_start = scan_pos;

            if let Some((tag_name, is_closing, is_self_closing, inner_start)) =
                parse_tag_at(open_start)
            {
                if !is_closing && !is_self_closing {
                    // Scan forward for the matching </tag_name>, tracking nesting depth.
                    let mut depth: usize = 1;
                    let mut fwd = inner_start;
                    let mut close_result: Option<(usize, usize)> = None;
                    while fwd < total_chars {
                        if ch(fwd) != '<' {
                            fwd += 1;
                            continue;
                        }
                        if let Some((tname, tclosing, tself, tend)) = parse_tag_at(fwd) {
                            if tname == tag_name {
                                if tclosing {
                                    depth -= 1;
                                    if depth == 0 {
                                        close_result = Some((fwd, tend));
                                        break;
                                    }
                                } else if !tself {
                                    depth += 1;
                                }
                            }
                            fwd = tend;
                        } else {
                            fwd += 1;
                        }
                    }

                    if let Some((close_start, close_end)) = close_result {
                        // Accept only if cursor is within this element's extent.
                        if cursor_pos >= open_start && cursor_pos < close_end {
                            return if modifier == 'i' {
                                if inner_start <= close_start {
                                    Some((inner_start, close_start))
                                } else {
                                    None
                                }
                            } else {
                                Some((open_start, close_end))
                            };
                        }
                    }
                }
            }

            // This '<' didn't yield an enclosing tag; keep scanning backward.
            if open_start == 0 {
                return None;
            }
            scan_pos = open_start - 1;
        }
    }

    /// Check if the active buffer is a LaTeX file.
    pub(crate) fn is_latex_buffer(&self) -> bool {
        self.active_buffer_state()
            .syntax
            .as_ref()
            .is_some_and(|s| s.language() == crate::core::syntax::SyntaxLanguage::Latex)
    }

    /// Find LaTeX \begin{env}...\end{env} text object range (ie/ae).
    pub(crate) fn find_latex_environment_object(
        &self,
        modifier: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        if !self.is_latex_buffer() {
            return None;
        }
        let total_chars = self.buffer().len_chars();
        if total_chars == 0 || cursor_pos >= total_chars {
            return None;
        }

        // Collect text into a string for substring search
        let text: String = self.buffer().content.chars().collect();

        // Find the enclosing \begin{name}...\end{name} pair.
        // Walk backward from cursor to find \begin{...}, tracking nesting.
        let mut scan = cursor_pos;
        loop {
            // Find previous \begin{ or \end{
            let before = &text[..=scan.min(text.len() - 1)];
            let begin_pos = before.rfind("\\begin{");
            let end_pos = before.rfind("\\end{");

            // If we find \end{ closer than \begin{, we need to skip over that
            // nested environment.
            match (begin_pos, end_pos) {
                (Some(bp), Some(ep)) if ep > bp => {
                    // \end{ is closer — this is a nested close, skip past it
                    if bp == 0 {
                        return None;
                    }
                    scan = bp.saturating_sub(1);
                    continue;
                }
                (Some(bp), _) => {
                    // Found a \begin{...} candidate
                    let env_name = self.latex_extract_env_name(&text, bp + 7)?;
                    let begin_end = bp + 7 + env_name.len() + 1; // past closing }

                    // Now find matching \end{env_name} forward, tracking nesting
                    let mut depth: usize = 1;
                    let mut fwd = begin_end;
                    while fwd < text.len() {
                        if text[fwd..].starts_with(&format!("\\begin{{{env_name}}}")) {
                            depth += 1;
                            fwd += 7 + env_name.len() + 1;
                        } else if text[fwd..].starts_with(&format!("\\end{{{env_name}}}")) {
                            depth -= 1;
                            if depth == 0 {
                                let end_start = fwd;
                                let end_end = fwd + 5 + env_name.len() + 1;
                                // Check cursor is within this range
                                if cursor_pos >= bp && cursor_pos < end_end {
                                    return if modifier == 'i' {
                                        Some((begin_end, end_start))
                                    } else {
                                        Some((bp, end_end))
                                    };
                                }
                                break;
                            }
                            fwd += 5 + env_name.len() + 1;
                        } else {
                            fwd += 1;
                        }
                    }
                    // This \begin didn't enclose cursor, try further back
                    if bp == 0 {
                        return None;
                    }
                    scan = bp - 1;
                }
                _ => return None,
            }
        }
    }

    /// Extract environment name from text starting at the position after `\begin{`.
    pub(crate) fn latex_extract_env_name(&self, text: &str, start: usize) -> Option<String> {
        let rest = text.get(start..)?;
        let end = rest.find('}')?;
        let name = &rest[..end];
        if name.is_empty() {
            return None;
        }
        Some(name.to_string())
    }

    /// Find LaTeX \command{...} text object range (ic/ac).
    /// `ic` selects the content inside braces, `ac` selects command + braces.
    pub(crate) fn find_latex_command_object(
        &self,
        modifier: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        let total_chars = self.buffer().len_chars();
        if total_chars == 0 || cursor_pos >= total_chars {
            return None;
        }

        let text: String = self.buffer().content.chars().collect();

        // If cursor is inside braces, walk back to find the command
        // First check if we're inside {...}
        let mut cmd_start;
        let mut brace_start = None;
        let mut depth: i32 = 0;
        for i in (0..=cursor_pos.min(text.len() - 1)).rev() {
            let c = text.as_bytes().get(i).copied().unwrap_or(0) as char;
            if c == '}' {
                depth += 1;
            } else if c == '{' {
                if depth == 0 {
                    brace_start = Some(i);
                    break;
                }
                depth -= 1;
            }
        }

        if let Some(bs) = brace_start {
            // Find the matching close brace
            let mut depth2: i32 = 1;
            let mut brace_end = None;
            for i in (bs + 1)..text.len() {
                let c = text.as_bytes().get(i).copied().unwrap_or(0) as char;
                if c == '{' {
                    depth2 += 1;
                } else if c == '}' {
                    depth2 -= 1;
                    if depth2 == 0 {
                        brace_end = Some(i + 1);
                        break;
                    }
                }
            }
            let brace_end = brace_end?;

            // Walk backward from '{' to find \command
            if bs > 0 {
                cmd_start = bs - 1;
                while cmd_start > 0 && text.as_bytes()[cmd_start].is_ascii_alphabetic() {
                    cmd_start -= 1;
                }
                if text.as_bytes()[cmd_start] == b'\\' {
                    return if modifier == 'i' {
                        Some((bs + 1, brace_end - 1))
                    } else {
                        Some((cmd_start, brace_end))
                    };
                }
            }
        }

        // Maybe cursor is on the \command itself — find the next { after it
        cmd_start = cursor_pos;
        while cmd_start > 0
            && text
                .as_bytes()
                .get(cmd_start)
                .is_some_and(|b| b.is_ascii_alphabetic())
        {
            cmd_start -= 1;
        }
        if text.as_bytes().get(cmd_start) == Some(&b'\\') {
            // Find the opening brace
            let mut pos = cmd_start + 1;
            while pos < text.len() && text.as_bytes()[pos].is_ascii_alphabetic() {
                pos += 1;
            }
            if text.as_bytes().get(pos) == Some(&b'{') {
                let bs = pos;
                let mut depth3: i32 = 1;
                let mut brace_end = None;
                for i in (bs + 1)..text.len() {
                    let c = text.as_bytes()[i] as char;
                    if c == '{' {
                        depth3 += 1;
                    } else if c == '}' {
                        depth3 -= 1;
                        if depth3 == 0 {
                            brace_end = Some(i + 1);
                            break;
                        }
                    }
                }
                let brace_end = brace_end?;
                return if modifier == 'i' {
                    Some((bs + 1, brace_end - 1))
                } else {
                    Some((cmd_start, brace_end))
                };
            }
        }

        None
    }

    /// Find LaTeX math text object range (i$/a$).
    /// Handles $...$, $$...$$, \(...\), \[...\].
    pub(crate) fn find_latex_math_object(
        &self,
        modifier: char,
        cursor_pos: usize,
    ) -> Option<(usize, usize)> {
        if !self.is_latex_buffer() {
            return None;
        }
        let total_chars = self.buffer().len_chars();
        if total_chars == 0 || cursor_pos >= total_chars {
            return None;
        }

        let text: String = self.buffer().content.chars().collect();
        let bytes = text.as_bytes();

        // Try \[...\] (display math)
        if let Some(result) =
            self.find_latex_delimited_pair(&text, cursor_pos, "\\[", "\\]", modifier)
        {
            return Some(result);
        }

        // Try \(...\) (inline math)
        if let Some(result) =
            self.find_latex_delimited_pair(&text, cursor_pos, "\\(", "\\)", modifier)
        {
            return Some(result);
        }

        // Try $$...$$ first (display math), then $...$ (inline math)
        // Scan for $ signs in the text, pairing them up
        let mut dollar_positions: Vec<(usize, bool)> = Vec::new(); // (pos, is_double)
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'$' {
                // Check for escaped \$
                if i > 0 && bytes[i - 1] == b'\\' {
                    i += 1;
                    continue;
                }
                if i + 1 < bytes.len() && bytes[i + 1] == b'$' {
                    dollar_positions.push((i, true));
                    i += 2;
                } else {
                    dollar_positions.push((i, false));
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        // Pair up dollars: each pair of same-type consecutive entries forms a math region
        let mut idx = 0;
        while idx + 1 < dollar_positions.len() {
            let (start, is_double_start) = dollar_positions[idx];
            let (end, is_double_end) = dollar_positions[idx + 1];
            if is_double_start == is_double_end {
                let delim_len = if is_double_start { 2 } else { 1 };
                let outer_start = start;
                let outer_end = end + delim_len;
                if cursor_pos >= outer_start && cursor_pos < outer_end {
                    return if modifier == 'i' {
                        Some((start + delim_len, end))
                    } else {
                        Some((outer_start, outer_end))
                    };
                }
                idx += 2;
            } else {
                idx += 1;
            }
        }

        None
    }

    /// Find a delimited pair like \[...\] or \(...\) around the cursor.
    pub(crate) fn find_latex_delimited_pair(
        &self,
        text: &str,
        cursor_pos: usize,
        open: &str,
        close: &str,
        modifier: char,
    ) -> Option<(usize, usize)> {
        // Search backward for the open delimiter
        for start in (0..=cursor_pos).rev() {
            if text[start..].starts_with(open) {
                let inner_start = start + open.len();
                // Search forward for matching close delimiter
                if let Some(rel) = text[inner_start..].find(close) {
                    let close_start = inner_start + rel;
                    let outer_end = close_start + close.len();
                    if cursor_pos >= start && cursor_pos < outer_end {
                        return if modifier == 'i' {
                            Some((inner_start, close_start))
                        } else {
                            Some((start, outer_end))
                        };
                    }
                }
                break; // Only check the nearest open delimiter
            }
        }
        None
    }

    /// Apply an operator to a text object
    pub(crate) fn apply_operator_text_object(
        &mut self,
        operator: char,
        modifier: char,
        obj_type: char,
        changed: &mut bool,
    ) {
        let cursor = self.view().cursor;
        let cursor_pos = self.buffer().line_to_char(cursor.line) + cursor.col;

        // Find text object range
        let range = match self.find_text_object_range(modifier, obj_type, cursor_pos) {
            Some(r) => r,
            None => return, // No matching text object found
        };

        let (start_pos, end_pos) = range;
        if start_pos >= end_pos {
            return;
        }

        // Get text content
        let text_content: String = self
            .buffer()
            .content
            .slice(start_pos..end_pos)
            .chars()
            .collect();

        let reg = self.active_register();
        self.set_register(reg, text_content, false);
        self.clear_selected_register();

        // Perform operation based on operator type
        match operator {
            'y' => {
                // Yank only - don't delete, don't change cursor
                // No undo group needed for yank
            }
            'd' | 'c' => {
                // Delete or change
                self.start_undo_group();
                self.delete_with_undo(start_pos, end_pos);

                // Move cursor to start of deletion
                let new_line = self.buffer().content.char_to_line(start_pos);
                let line_start = self.buffer().line_to_char(new_line);
                let new_col = start_pos - line_start;
                self.view_mut().cursor.line = new_line;
                self.view_mut().cursor.col = new_col;

                *changed = true;

                // If operator is 'c', enter insert mode
                if operator == 'c' {
                    self.mode = Mode::Insert;
                    self.count = None;
                    // Don't finish_undo_group - let insert mode do it
                    // Don't clamp cursor - insert mode allows cursor at end of line
                } else {
                    self.clamp_cursor_col();
                    self.finish_undo_group();
                }
            }
            'q' | 'Q' => {
                // gq/gw format — convert char range to line range
                let start_line = self.buffer().content.char_to_line(start_pos);
                let end_line = {
                    let l = self
                        .buffer()
                        .content
                        .char_to_line(end_pos.saturating_sub(1).max(start_pos));
                    l
                };
                let save_cursor = self.view().cursor;
                self.format_lines(start_line, end_line, changed);
                if operator == 'Q' {
                    // gw: restore cursor position
                    self.view_mut().cursor = save_cursor;
                    self.clamp_cursor_col();
                }
            }
            '~' | 'u' | 'U' => {
                self.apply_case_range(start_pos, end_pos, operator, changed);
            }
            'R' => {
                // g?: ROT13 encode
                self.apply_rot13_range(start_pos, end_pos, changed);
            }
            '>' | '<' | '=' => {
                let start_line = self.buffer().content.char_to_line(start_pos);
                let end_line = self
                    .buffer()
                    .content
                    .char_to_line(end_pos.saturating_sub(1).max(start_pos));
                let count = end_line - start_line + 1;
                if operator == '>' {
                    self.indent_lines(start_line, count, changed);
                } else if operator == '<' {
                    self.dedent_lines(start_line, count, changed);
                } else {
                    self.auto_indent_lines(start_line, count, changed);
                }
            }
            '!' => {
                let start_line = self.buffer().content.char_to_line(start_pos);
                let end_line = self
                    .buffer()
                    .content
                    .char_to_line(end_pos.saturating_sub(1).max(start_pos));
                self.mode = Mode::Command;
                self.command_buffer = format!("{},{}!", start_line + 1, end_line + 1);
                self.command_cursor = self.command_buffer.chars().count();
            }
            _ => {
                // Unknown operator - do nothing
            }
        }
    }

    // --- Line operations ---

    #[allow(dead_code)]
    pub(crate) fn delete_current_line(&mut self, changed: &mut bool) {
        self.delete_lines(1, changed);
    }

    /// Delete count lines starting from current line
    pub(crate) fn delete_lines(&mut self, count: usize, changed: &mut bool) {
        let num_lines = self.buffer().len_lines();
        if num_lines == 0 {
            return;
        }

        let start_line = self.view().cursor.line;
        let end_line = (start_line + count).min(num_lines);
        let actual_count = end_line - start_line;

        if actual_count == 0 {
            return;
        }

        let line_start = self.buffer().line_to_char(start_line);
        let line_end = if end_line < num_lines {
            self.buffer().line_to_char(end_line)
        } else {
            self.buffer().len_chars()
        };

        // Save deleted lines to register (linewise)
        let deleted_content: String = self
            .buffer()
            .content
            .slice(line_start..line_end)
            .chars()
            .collect();

        // Ensure linewise content ends with newline
        let deleted_content = if deleted_content.ends_with('\n') {
            deleted_content
        } else {
            format!("{}\n", deleted_content)
        };
        let reg = self.active_register();
        self.set_delete_register(reg, deleted_content, true);
        self.clear_selected_register();

        // Determine what to delete
        let (delete_start, delete_end) = if end_line < num_lines {
            // Delete lines including their newlines
            (line_start, line_end)
        } else {
            // Deleting to end of buffer
            if start_line > 0 {
                // Delete the newline before the first line being deleted
                (line_start - 1, line_end)
            } else {
                (line_start, line_end)
            }
        };

        self.delete_with_undo(delete_start, delete_end);
        *changed = true;

        let new_num_lines = self.buffer().len_lines();
        if self.view().cursor.line >= new_num_lines && new_num_lines > 0 {
            self.view_mut().cursor.line = new_num_lines - 1;
        }
        self.view_mut().cursor.col = 0;
        self.clamp_cursor_col();
    }

    #[allow(dead_code)]
    pub(crate) fn delete_to_end_of_line(&mut self, changed: &mut bool) {
        self.delete_to_end_of_line_with_count(1, changed);
    }

    pub(crate) fn delete_to_end_of_line_with_count(&mut self, count: usize, changed: &mut bool) {
        let start_line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let char_idx = self.buffer().line_to_char(start_line) + col;

        if count == 1 {
            // Single D: delete to end of current line, excluding newline
            let line_content = self.buffer().content.line(start_line);
            let line_start = self.buffer().line_to_char(start_line);
            let line_end = line_start + line_content.len_chars();

            let delete_end = if line_content.chars().last() == Some('\n') {
                line_end - 1
            } else {
                line_end
            };

            if char_idx < delete_end {
                let deleted_content: String = self
                    .buffer()
                    .content
                    .slice(char_idx..delete_end)
                    .chars()
                    .collect();
                let reg = self.active_register();
                self.set_register(reg, deleted_content, false);
                self.clear_selected_register();

                self.delete_with_undo(char_idx, delete_end);
                self.clamp_cursor_col();
                *changed = true;
            }
        } else {
            // Multiple D: delete to end of current line (excluding newline) + (count-1) full lines below
            let total_lines = self.buffer().len_lines();
            let line_content = self.buffer().content.line(start_line);
            let line_start = self.buffer().line_to_char(start_line);
            let line_end = line_start + line_content.len_chars();

            // End of current line excluding newline
            let first_part_end = if line_content.chars().last() == Some('\n') {
                line_end - 1
            } else {
                line_end
            };

            // Build the content to delete (for register)
            let to_eol: String = self
                .buffer()
                .content
                .slice(char_idx..first_part_end)
                .chars()
                .collect();

            let mut deleted_content = to_eol;
            deleted_content.push('\n');

            // Add (count-1) full lines
            if count > 1 {
                let last_line = (start_line + count - 1).min(total_lines - 1);
                let lines_start = line_end; // After newline of current line
                let lines_end = if last_line + 1 < total_lines {
                    self.buffer().line_to_char(last_line + 1)
                } else {
                    self.buffer().len_chars()
                };

                let full_lines: String = self
                    .buffer()
                    .content
                    .slice(lines_start..lines_end)
                    .chars()
                    .collect();
                deleted_content.push_str(&full_lines);
            }

            let reg = self.active_register();
            self.set_register(reg, deleted_content, false);
            self.clear_selected_register();

            // Perform the actual deletion: from char_idx to first_part_end
            self.delete_with_undo(char_idx, first_part_end);

            // Now delete the (count-1) full lines that follow
            if count > 1 {
                // After deleting to EOL, the cursor position hasn't moved
                // The newline is at char_idx, and we want to delete starting from char_idx + 1
                let lines_to_delete = count - 1;
                let delete_from = char_idx + 1; // Start after the newline

                // Calculate how many chars to delete
                let remaining_lines = self.buffer().len_lines() - start_line - 1;
                let actual_lines_to_delete = lines_to_delete.min(remaining_lines);

                if actual_lines_to_delete > 0 {
                    let delete_to =
                        if start_line + 1 + actual_lines_to_delete < self.buffer().len_lines() {
                            self.buffer()
                                .line_to_char(start_line + 1 + actual_lines_to_delete)
                        } else {
                            self.buffer().len_chars()
                        };

                    if delete_from < delete_to {
                        self.delete_with_undo(delete_from, delete_to);
                    }
                }
            }

            self.clamp_cursor_col();
            *changed = true;
        }
    }

    pub(crate) fn move_left(&mut self) {
        if self.view().cursor.col > 0 {
            self.view_mut().cursor.col -= 1;
        }
    }

    pub(crate) fn move_down(&mut self) {
        let max_line = self.buffer().len_lines().saturating_sub(1);
        let mut next = self.view().cursor.line;
        loop {
            if next >= max_line {
                return;
            }
            next += 1;
            if !self.view().is_line_hidden(next) {
                break;
            }
        }
        self.view_mut().cursor.line = next;
        self.clamp_cursor_col();
    }

    pub(crate) fn move_up(&mut self) {
        let mut prev = self.view().cursor.line;
        loop {
            if prev == 0 {
                return;
            }
            prev -= 1;
            if !self.view().is_line_hidden(prev) {
                break;
            }
        }
        self.view_mut().cursor.line = prev;
        self.clamp_cursor_col();
    }

    // ── Indent / completion helpers ───────────────────────────────────────────

    /// Compute the indent string for a new line inserted after `line_idx`.
    /// When `auto_indent` is on this copies the previous line's indent *and*
    /// adds an extra indent level when the line ends with an indent-trigger
    /// (language-aware via `line_triggers_indent`).
    pub(crate) fn smart_indent_for_newline(&self, line_idx: usize) -> String {
        if !self.settings.auto_indent {
            return String::new();
        }
        let base = self.get_line_indent_str(line_idx);
        let line_text: String = self.buffer().content.line(line_idx).chars().collect();
        let trimmed = line_text.trim_end_matches(['\n', '\r']);

        if self.line_triggers_indent(trimmed) {
            let sw = self.effective_shift_width();
            let extra = if self.settings.expand_tab {
                " ".repeat(sw)
            } else {
                "\t".to_string()
            };
            format!("{}{}", base, extra)
        } else {
            base
        }
    }

    /// Check whether a closing character (`}`, `)`, `]`) just typed on a
    /// line that was previously only whitespace should auto-outdent (reduce
    /// indent by one `shift_width`).  Called *after* the character has been
    /// inserted.  Returns the new indent string if outdenting is appropriate,
    /// or `None` to leave indent unchanged.
    pub(crate) fn auto_outdent_for_closing(&self, line_idx: usize) -> Option<String> {
        if !self.settings.auto_indent {
            return None;
        }
        let line_text: String = self.buffer().content.line(line_idx).chars().collect();
        let trimmed = line_text.trim_end_matches(['\n', '\r']);
        // The closing bracket is already inserted.  Outdent only if
        // everything before it is whitespace (i.e. it's the first
        // non-blank character on the line).
        let before = trimmed.trim_end_matches(['}', ')', ']']);
        if !before.chars().all(|c| c == ' ' || c == '\t') {
            return None;
        }
        let sw = self.effective_shift_width();
        let cur_indent = self.get_line_indent_str(line_idx);
        if cur_indent.len() >= sw {
            let new_len = cur_indent.len() - sw;
            if self.settings.expand_tab {
                Some(" ".repeat(new_len))
            } else {
                Some(cur_indent[..cur_indent.len().saturating_sub(1)].to_string())
            }
        } else {
            Some(String::new())
        }
    }

    /// Return the leading whitespace string (spaces/tabs) of the given buffer line.
    pub(crate) fn get_line_indent_str(&self, line_idx: usize) -> String {
        let total = self.buffer().len_lines();
        if line_idx >= total {
            return String::new();
        }
        self.buffer()
            .content
            .line(line_idx)
            .chars()
            .take_while(|&c| c == ' ' || c == '\t')
            .collect()
    }

    /// Return the effective shift width for the active buffer.
    /// Uses the buffer's auto-detected indent width if available,
    /// otherwise falls back to `settings.shift_width`.
    pub(crate) fn effective_shift_width(&self) -> usize {
        self.buffer_manager
            .get(self.active_buffer_id())
            .and_then(|s| s.detected_indent)
            .map(|n| n as usize)
            .unwrap_or(self.settings.shift_width as usize)
    }

    /// True for word characters: [a-zA-Z0-9_].
    pub(crate) fn is_word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    /// Walk left from cursor to find the current word prefix.
    /// Returns `(prefix, start_col)` where `start_col` is the column index
    /// where the prefix begins.
    pub(crate) fn completion_prefix_at_cursor(&self) -> (String, usize) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let chars: Vec<char> = self.buffer().content.line(line).chars().collect();
        // Clamp col to valid range — cursor can be past end after edits or
        // on lines shorter than expected (e.g. trailing newline excluded).
        let col = col.min(chars.len());
        let mut start = col;
        while start > 0 && Self::is_word_char(chars[start - 1]) {
            start -= 1;
        }
        let prefix: String = chars[start..col].iter().collect();
        (prefix, start)
    }

    /// Fast word completion: scan only ~500 lines around the cursor.
    /// Used by auto-popup to avoid O(N) scan on every keystroke.
    pub(crate) fn word_completions_nearby(&self, prefix: &str) -> Vec<String> {
        let total = self.buffer().len_lines();
        let cursor_line = self.view().cursor.line;
        let radius = 250usize;
        let start = cursor_line.saturating_sub(radius);
        let end = (cursor_line + radius).min(total);
        let mut set: std::collections::HashSet<String> = Default::default();
        for line_idx in start..end {
            let text: String = self.buffer().content.line(line_idx).chars().collect();
            let chars: Vec<char> = text.chars().collect();
            let len = chars.len();
            let mut i = 0usize;
            while i < len {
                if Self::is_word_char(chars[i]) {
                    let word_start = i;
                    while i < len && Self::is_word_char(chars[i]) {
                        i += 1;
                    }
                    let word: String = chars[word_start..i].iter().collect();
                    if word.starts_with(prefix) && word != prefix {
                        set.insert(word);
                    }
                } else {
                    i += 1;
                }
            }
        }
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        v
    }

    /// Collect all words in the current buffer that start with `prefix`,
    /// deduplicated, sorted, excluding an exact match of `prefix` itself.
    /// Used by Ctrl-N/Ctrl-P (manual completion) which can afford the full scan.
    pub(crate) fn word_completions_for_prefix(&self, prefix: &str) -> Vec<String> {
        let mut set: std::collections::HashSet<String> = Default::default();
        for line_idx in 0..self.buffer().len_lines() {
            let text: String = self.buffer().content.line(line_idx).chars().collect();
            let chars: Vec<char> = text.chars().collect();
            let len = chars.len();
            let mut i = 0usize;
            while i < len {
                if Self::is_word_char(chars[i]) {
                    let start = i;
                    while i < len && Self::is_word_char(chars[i]) {
                        i += 1;
                    }
                    let word: String = chars[start..i].iter().collect();
                    if word.starts_with(prefix) && word != prefix {
                        set.insert(word);
                    }
                } else {
                    i += 1;
                }
            }
        }
        let mut v: Vec<String> = set.into_iter().collect();
        v.sort();
        v
    }

    /// Delete the previously inserted candidate (or prefix), insert the new
    /// candidate at `completion_start_col`, and update the cursor column.
    pub(crate) fn apply_completion_candidate(&mut self, idx: usize) {
        let line = self.view().cursor.line;
        let prev_end = self.view().cursor.col;
        let start = self.completion_start_col;
        let line_char = self.buffer().line_to_char(line);
        if prev_end > start {
            self.delete_with_undo(line_char + start, line_char + prev_end);
        }
        let candidate = self.completion_candidates[idx].clone();
        self.insert_with_undo(line_char + start, &candidate);
        self.view_mut().cursor.col = start + candidate.len();
    }

    /// Dismiss the completion popup and cancel any pending LSP completion request.
    /// This ensures that a late-arriving LSP response cannot re-show a popup
    /// after the user has already dismissed it (e.g. by pressing Escape or
    /// moving the cursor).
    pub(crate) fn dismiss_completion(&mut self) {
        self.completion_candidates.clear();
        self.completion_idx = None;
        self.completion_display_only = false;
        self.lsp_pending_completion = None;
    }

    /// Trigger auto-popup completion based on current cursor prefix.
    /// Called after each text change in Insert mode.
    pub(crate) fn trigger_auto_completion(&mut self) {
        let (prefix, _) = self.completion_prefix_at_cursor();
        if prefix.is_empty() {
            self.dismiss_completion();
            return;
        }
        // Use a fast nearby-lines scan instead of scanning the entire buffer.
        // For a 15K-line file, full scan takes 270ms; nearby scan is ~1ms.
        let candidates = self.word_completions_nearby(&prefix);
        if !candidates.is_empty() {
            self.completion_start_col = self.view().cursor.col - prefix.chars().count();
            self.completion_candidates = candidates;
            self.completion_idx = Some(0);
            self.completion_display_only = true;
        } else {
            // No buffer-word hits yet; clear popup but keep LSP pending
            self.completion_candidates.clear();
            self.completion_idx = None;
            self.completion_display_only = false;
        }
        // Async LSP source — response will update candidates if popup is still active
        self.lsp_request_completion();
    }

    // ── Fold helpers ──────────────────────────────────────────────────────────

    /// Count leading whitespace characters (spaces = 1, tabs = tab_width).
    pub(crate) fn line_indent(&self, line_idx: usize) -> usize {
        let total = self.buffer().len_lines();
        if line_idx >= total {
            return 0;
        }
        let line = self.buffer().content.line(line_idx);
        let tab_width = 4usize;
        let mut indent = 0usize;
        for ch in line.chars() {
            match ch {
                ' ' => indent += 1,
                '\t' => indent += tab_width,
                _ => break,
            }
        }
        indent
    }

    /// Detect the fold range starting at `start_line` using indentation heuristics.
    /// Returns `Some((start, end))` when at least one following line has strictly
    /// greater indentation. Returns `None` for blank/empty trailing sections.
    pub(crate) fn detect_fold_range(&self, start_line: usize) -> Option<(usize, usize)> {
        let total = self.buffer().len_lines();
        if start_line + 1 >= total {
            return None;
        }
        let base_indent = self.line_indent(start_line);
        let mut end = start_line;
        for idx in (start_line + 1)..total {
            let line = self.buffer().content.line(idx);
            let text: String = line.chars().collect();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                // blank lines are included in fold body
                end = idx;
                continue;
            }
            if self.line_indent(idx) > base_indent {
                end = idx;
            } else {
                break;
            }
        }
        if end > start_line {
            Some((start_line, end))
        } else {
            None
        }
    }

    /// Toggle the fold at `line_idx` regardless of cursor position.
    /// Used by click handlers when the user clicks the fold indicator.
    pub fn toggle_fold_at_line(&mut self, line_idx: usize) {
        if self.view().fold_at(line_idx).is_some() {
            self.view_mut().open_fold(line_idx);
        } else {
            let saved = self.view().cursor.line;
            self.view_mut().cursor.line = line_idx;
            self.cmd_fold_close();
            self.view_mut().cursor.line = saved;
        }
    }

    pub(crate) fn cmd_fold_toggle(&mut self) {
        let line = self.view().cursor.line;
        if self.view().fold_at(line).is_some() {
            self.view_mut().open_fold(line);
        } else {
            self.cmd_fold_close();
        }
    }

    pub(crate) fn cmd_fold_close(&mut self) {
        let line = self.view().cursor.line;
        if let Some((start, end)) = self.detect_fold_range(line) {
            self.view_mut().close_fold(start, end);
            // If cursor ended up inside the fold, move it to the header.
            if self.view().is_line_hidden(self.view().cursor.line) {
                self.view_mut().cursor.line = start;
                self.clamp_cursor_col();
            }
        }
    }

    /// Find the enclosing foldable block for `line` by walking upward to find
    /// a line with strictly less indentation, then using `detect_fold_range`.
    pub(crate) fn find_enclosing_fold_range(&self, line: usize) -> Option<(usize, usize)> {
        let cur_indent = self.line_indent(line);
        // Walk upward to find a line with strictly less indentation.
        for idx in (0..line).rev() {
            let text: String = self.buffer().content.line(idx).chars().collect();
            if text.trim().is_empty() {
                continue;
            }
            if self.line_indent(idx) < cur_indent {
                // Found a candidate header — verify it can fold over our line.
                if let Some((start, end)) = self.detect_fold_range(idx) {
                    if end >= line {
                        return Some((start, end));
                    }
                }
                // Keep walking — this line's fold range didn't cover us.
            }
        }
        None
    }

    /// Progressive fold (VSCode Ctrl+Shift+[): fold the enclosing block around
    /// the cursor.  If the cursor is already on a fold header, fold the parent
    /// block instead.  This makes repeated presses fold progressively larger
    /// regions.
    pub(crate) fn cmd_fold_close_progressive(&mut self) {
        let line = self.view().cursor.line;

        // If cursor is on a fold header, look for a parent fold.
        if self.view().fold_at(line).is_some() {
            if let Some((start, end)) = self.find_enclosing_fold_range(line) {
                self.view_mut().close_fold(start, end);
                self.view_mut().cursor.line = start;
                self.clamp_cursor_col();
            }
            return;
        }

        // First try: fold starting at cursor line (cursor is on a header).
        if let Some((start, end)) = self.detect_fold_range(line) {
            self.view_mut().close_fold(start, end);
            if self.view().is_line_hidden(self.view().cursor.line) {
                self.view_mut().cursor.line = start;
                self.clamp_cursor_col();
            }
            return;
        }

        // Second try: cursor is inside a block body — find enclosing fold.
        if let Some((start, end)) = self.find_enclosing_fold_range(line) {
            self.view_mut().close_fold(start, end);
            self.view_mut().cursor.line = start;
            self.clamp_cursor_col();
        }
    }

    /// Progressive unfold: if cursor is on a fold header, open it.  If cursor
    /// is NOT on a fold header but is inside a visible region that contains
    /// nested folds, open the nearest inner fold. This makes repeated
    /// Ctrl+Shift+] unfold progressively (VSCode behavior).
    pub(crate) fn cmd_fold_open_progressive(&mut self) {
        let line = self.view().cursor.line;
        if self.view().fold_at(line).is_some() {
            // Cursor is on a fold header — open just this fold.
            self.view_mut().open_fold(line);
        } else {
            // Check if there are any folds whose header is at or after cursor
            // line (the nearest fold below cursor).  This handles the case where
            // the user pressed unfold on a parent line after folding children.
            let nearest = self
                .view()
                .folds
                .iter()
                .find(|f| f.start >= line)
                .map(|f| f.start);
            if let Some(fold_line) = nearest {
                self.view_mut().open_fold(fold_line);
            }
        }
    }

    pub(crate) fn cmd_fold_open(&mut self) {
        let line = self.view().cursor.line;
        self.view_mut().open_fold(line);
    }

    /// zM — close all folds in the buffer using indent-based detection.
    pub(crate) fn cmd_fold_close_all(&mut self) {
        let total = self.buffer().len_lines();
        let mut i = 0;
        while i < total {
            if let Some((start, end)) = self.detect_fold_range(i) {
                self.view_mut().close_fold(start, end);
                i = end + 1;
            } else {
                i += 1;
            }
        }
        // Clamp cursor if it ended up hidden.
        let cursor_line = self.view().cursor.line;
        if self.view().is_line_hidden(cursor_line) {
            // Move cursor to the nearest fold header above.
            for f in self.view().folds.iter().rev() {
                if f.start <= cursor_line && cursor_line <= f.end {
                    self.view_mut().cursor.line = f.start;
                    break;
                }
            }
            self.clamp_cursor_col();
        }
    }

    /// zA — toggle fold recursively at cursor.
    pub(crate) fn cmd_fold_toggle_recursive(&mut self) {
        let line = self.view().cursor.line;
        if let Some(fold) = self.view().fold_at(line).cloned() {
            // Open this fold and any folds inside it.
            self.view_mut().open_folds_in_range(fold.start, fold.end);
        } else {
            self.cmd_fold_close();
        }
    }

    /// zO — open fold at cursor recursively (open all nested folds).
    pub(crate) fn cmd_fold_open_recursive(&mut self) {
        let line = self.view().cursor.line;
        if let Some(fold) = self.view().fold_at(line).cloned() {
            self.view_mut().open_folds_in_range(fold.start, fold.end);
        } else {
            // Also check if cursor is on a line that *could* fold.
            self.view_mut().open_fold(line);
        }
    }

    /// zC — close fold at cursor recursively. (Flat model: same as zc.)
    pub(crate) fn cmd_fold_close_recursive(&mut self) {
        self.cmd_fold_close();
    }

    /// zd — delete fold at cursor.
    pub(crate) fn cmd_fold_delete(&mut self) {
        let line = self.view().cursor.line;
        if !self.view_mut().delete_fold_at(line) {
            self.message = "E490: No fold found".to_string();
        }
    }

    /// zD — delete fold at cursor recursively (including nested).
    pub(crate) fn cmd_fold_delete_recursive(&mut self) {
        let line = self.view().cursor.line;
        if let Some(fold) = self.view().fold_at(line).cloned() {
            self.view_mut().delete_folds_in_range(fold.start, fold.end);
        } else if !self.view_mut().delete_fold_at(line) {
            self.message = "E490: No fold found".to_string();
        }
    }

    /// Used by zf{motion} and zF — create a fold for the given line range.
    pub(crate) fn cmd_fold_create(&mut self, start: usize, end: usize) {
        if end <= start {
            return;
        }
        self.view_mut().close_fold(start, end);
        let lines = end - start;
        self.message = format!("{lines} lines folded");
    }

    /// zv — open enough folds to make cursor line visible.
    pub(crate) fn cmd_fold_open_cursor_visible(&mut self) {
        loop {
            let cursor_line = self.view().cursor.line;
            let fold = self
                .view()
                .folds
                .iter()
                .find(|f| cursor_line > f.start && cursor_line <= f.end)
                .cloned();
            if let Some(f) = fold {
                self.view_mut().open_fold(f.start);
            } else {
                break;
            }
        }
    }

    /// zx — recompute folds: open all, then close all.
    pub(crate) fn cmd_fold_recompute(&mut self) {
        self.view_mut().open_all_folds();
        self.cmd_fold_close_all();
    }

    /// zj — move to the start of the next fold.
    pub(crate) fn cmd_fold_move_next(&mut self) {
        let cursor_line = self.view().cursor.line;
        let total = self.buffer().len_lines();
        // First check existing closed folds.
        let next_fold = self
            .view()
            .folds
            .iter()
            .find(|f| f.start > cursor_line)
            .map(|f| f.start);
        // Also scan for potential fold starts (lines with children indented deeper).
        let mut next_detectable = None;
        for i in (cursor_line + 1)..total {
            if self.view().is_line_hidden(i) {
                continue;
            }
            if self.detect_fold_range(i).is_some() {
                next_detectable = Some(i);
                break;
            }
        }
        let target = match (next_fold, next_detectable) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        if let Some(line) = target {
            self.view_mut().cursor.line = line;
            self.view_mut().cursor.col = 0;
        }
    }

    /// zk — move to the end of the previous fold.
    pub(crate) fn cmd_fold_move_prev(&mut self) {
        let cursor_line = self.view().cursor.line;
        // Check existing closed folds.
        let prev_fold = self
            .view()
            .folds
            .iter()
            .rev()
            .find(|f| f.start < cursor_line)
            .map(|f| f.start);
        // Also scan for potential fold starts.
        let mut prev_detectable = None;
        for i in (0..cursor_line).rev() {
            if self.view().is_line_hidden(i) {
                continue;
            }
            if self.detect_fold_range(i).is_some() {
                prev_detectable = Some(i);
                break;
            }
        }
        let target = match (prev_fold, prev_detectable) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        if let Some(line) = target {
            self.view_mut().cursor.line = line;
            self.view_mut().cursor.col = 0;
        }
    }

    /// z<CR> — scroll cursor line to top, then move to first non-blank.
    pub(crate) fn scroll_cursor_top_first_nonblank(&mut self) {
        self.scroll_cursor_top();
        let line = self.view().cursor.line;
        self.view_mut().cursor.col = self.first_non_blank_col(line);
    }

    /// z. — scroll cursor line to center, then move to first non-blank.
    pub(crate) fn scroll_cursor_center_first_nonblank(&mut self) {
        self.scroll_cursor_center();
        let line = self.view().cursor.line;
        self.view_mut().cursor.col = self.first_non_blank_col(line);
    }

    /// z- — scroll cursor line to bottom, then move to first non-blank.
    pub(crate) fn scroll_cursor_bottom_first_nonblank(&mut self) {
        self.scroll_cursor_bottom();
        let line = self.view().cursor.line;
        self.view_mut().cursor.col = self.first_non_blank_col(line);
    }

    /// zh — scroll view left by `count` columns.
    pub(crate) fn scroll_left_by(&mut self, count: usize) {
        let sl = self.view().scroll_left;
        self.view_mut().scroll_left = sl.saturating_sub(count);
    }

    /// zl — scroll view right by `count` columns.
    pub(crate) fn scroll_right_by(&mut self, count: usize) {
        self.view_mut().scroll_left += count;
    }

    /// zH — scroll half screen width left.
    pub(crate) fn scroll_left_half_screen(&mut self) {
        let half = self.view().viewport_cols / 2;
        let half = if half == 0 { 1 } else { half };
        self.scroll_left_by(half);
    }

    /// zL — scroll half screen width right.
    pub(crate) fn scroll_right_half_screen(&mut self) {
        let half = self.view().viewport_cols / 2;
        let half = if half == 0 { 1 } else { half };
        self.scroll_right_by(half);
    }

    pub(crate) fn move_right(&mut self) {
        let line = self.view().cursor.line;
        let max_valid_col = self.get_max_cursor_col(line);
        if self.view().cursor.col < max_valid_col {
            self.view_mut().cursor.col += 1;
        }
    }

    pub(crate) fn move_right_insert(&mut self) {
        let line = self.view().cursor.line;
        let max = self.get_line_len_for_insert(line);
        if self.view().cursor.col < max {
            self.view_mut().cursor.col += 1;
        }
    }

    pub(crate) fn get_line_len_for_insert(&self, line_idx: usize) -> usize {
        let len = self.buffer().line_len_chars(line_idx);
        if len == 0 {
            return 0;
        }
        let line = self.buffer().content.line(line_idx);
        if line.chars().last() == Some('\n') {
            len - 1
        } else {
            len
        }
    }

    pub(crate) fn clamp_cursor_col_insert(&mut self) {
        let line = self.view().cursor.line;
        let max = self.get_line_len_for_insert(line);
        if self.view().cursor.col > max {
            self.view_mut().cursor.col = max;
        }
    }

    // --- Register operations ---

    /// Returns the active register name (selected or default '"').
    pub(crate) fn active_register(&self) -> char {
        self.selected_register.unwrap_or('"')
    }

    /// Sets a register's content. `is_linewise` affects paste behavior.
    /// For `+` and `*` registers, also writes to the system clipboard.
    pub(crate) fn set_register(&mut self, reg: char, content: String, is_linewise: bool) {
        self.registers.insert(reg, (content.clone(), is_linewise));
        // Also copy to unnamed register if using a named register
        if reg != '"' {
            self.registers.insert('"', (content.clone(), is_linewise));
        }
        // Sync clipboard registers to system clipboard
        if reg == '+' || reg == '*' {
            if let Some(ref cb_write) = self.clipboard_write {
                if let Err(e) = cb_write(&content) {
                    self.message = format!("Clipboard write failed: {}", e);
                }
            }
        }
    }

    /// Gets a register's content and linewise flag (borrowed).
    pub(crate) fn get_register(&self, reg: char) -> Option<&(String, bool)> {
        self.registers.get(&reg)
    }

    /// Sets a yank register. Like set_register, but ALSO always updates "0.
    pub(crate) fn set_yank_register(&mut self, reg: char, content: String, is_linewise: bool) {
        self.set_register(reg, content.clone(), is_linewise);
        // "0 is the yank-only register — set on every yank, never on deletes.
        self.registers.insert('0', (content, is_linewise));
    }

    /// Sets a delete register. Like set_register, but:
    /// - Linewise / multi-line: shifts "1"-"8" → "2"-"9", sets "1".
    /// - Character (< 1 line): sets "-" (small-delete register).
    pub(crate) fn set_delete_register(&mut self, reg: char, content: String, is_linewise: bool) {
        self.set_register(reg, content.clone(), is_linewise);
        if is_linewise || content.contains('\n') {
            // Multi-line delete: shift numbered registers down
            for i in (1usize..=8).rev() {
                let from = char::from_digit(i as u32, 10).unwrap();
                let to = char::from_digit((i + 1) as u32, 10).unwrap();
                if let Some(val) = self.registers.get(&from).cloned() {
                    self.registers.insert(to, val);
                }
            }
            self.registers.insert('1', (content, is_linewise));
        } else if !content.is_empty() {
            // Small character delete: set "-" register
            self.registers.insert('-', (content, false));
        }
    }

    /// Gets register content as owned data.
    /// For `+` and `*` registers, reads from the system clipboard.
    /// For `%`, `/`, `.` read-only registers, returns the appropriate value.
    pub fn get_register_content(&mut self, reg: char) -> Option<(String, bool)> {
        match reg {
            '+' | '*' => {
                if let Some(ref cb_read) = self.clipboard_read {
                    match cb_read() {
                        Ok(text) => return Some((text, false)),
                        Err(e) => {
                            self.message = format!("Clipboard read failed: {}", e);
                        }
                    }
                }
                // Fall back to internal register if clipboard unavailable
                self.registers.get(&reg).cloned()
            }
            '%' => {
                // Current filename (read-only)
                let name = self
                    .active_buffer_state()
                    .file_path
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Some((name, false))
            }
            '/' => {
                // Last search pattern (read-only)
                Some((self.search_query.clone(), false))
            }
            '.' => {
                // Last inserted text (read-only)
                Some((self.last_inserted_text.clone(), false))
            }
            _ => self.registers.get(&reg).cloned(),
        }
    }

    /// Clears the selected register after an operation.
    pub(crate) fn clear_selected_register(&mut self) {
        self.selected_register = None;
    }

    /// Records a yank highlight region for brief visual feedback.
    /// `end` is the inclusive last cursor position of the yanked range.
    pub(crate) fn record_yank_highlight(&mut self, start: Cursor, end: Cursor, is_linewise: bool) {
        self.yank_highlight = Some((start, end, is_linewise));
    }

    /// Clears the yank highlight. Called by the UI backend after ~200 ms.
    pub fn clear_yank_highlight(&mut self) {
        self.yank_highlight = None;
    }

    // --- Macro operations ---

    /// Encode a keystroke for macro recording using Vim-style notation.
    /// Returns a string representation that can be decoded during playback.
    pub(crate) fn encode_key_for_macro(
        &self,
        key_name: &str,
        unicode: Option<char>,
        ctrl: bool,
    ) -> String {
        // Handle Ctrl combinations
        if ctrl {
            if let Some(ch) = unicode {
                // Ctrl-D, Ctrl-U, etc.
                return format!("<C-{}>", ch.to_uppercase());
            }
        }

        // Handle special keys (no unicode)
        if unicode.is_none() {
            match key_name {
                "Escape" => return "\x1b".to_string(),
                "Return" => return "<CR>".to_string(),
                "BackSpace" => return "<BS>".to_string(),
                "Delete" => return "<Del>".to_string(),
                "Left" => return "<Left>".to_string(),
                "Right" => return "<Right>".to_string(),
                "Up" => return "<Up>".to_string(),
                "Down" => return "<Down>".to_string(),
                "Home" => return "<Home>".to_string(),
                "End" => return "<End>".to_string(),
                "Page_Up" => return "<PageUp>".to_string(),
                "Page_Down" => return "<PageDown>".to_string(),
                _ => return String::new(), // Unknown key, don't record
            }
        }

        // Regular character
        if let Some(ch) = unicode {
            ch.to_string()
        } else {
            String::new()
        }
    }

    /// Start recording a macro into the specified register.
    pub(crate) fn start_macro_recording(&mut self, register: char) {
        self.macro_recording = Some(register);
        self.recording_buffer.clear();
        self.message = format!("Recording macro into register '{}'", register);
    }

    /// Stop recording and save the macro to the register.
    pub(crate) fn stop_macro_recording(&mut self) {
        if let Some(reg) = self.macro_recording {
            // Convert recording_buffer to string
            let macro_content: String = self.recording_buffer.iter().collect();

            // Store in register (not linewise)
            self.set_register(reg, macro_content, false);

            self.message = format!("Macro recorded into register '{}'", reg);
            self.macro_recording = None;
            self.recording_buffer.clear();
        }
    }

    /// Play a macro from the specified register.
    pub(crate) fn play_macro(&mut self, register: char) -> Result<(), String> {
        // Check recursion depth
        if self.macro_recursion_depth >= MAX_MACRO_RECURSION {
            return Err("Macro recursion too deep".to_string());
        }

        // Get macro content from register (clone it to avoid borrow issues)
        let content = if let Some((content, _)) = self.get_register(register) {
            content.clone()
        } else {
            self.message = format!("Register '{}' is empty", register);
            return Ok(());
        };

        if content.is_empty() {
            self.message = format!("Register '{}' is empty", register);
            return Ok(());
        }

        // Remember last macro for @@
        self.last_macro_register = Some(register);

        // Add keys to playback queue
        for ch in content.chars() {
            self.macro_playback_queue.push_back(ch);
        }

        self.message = format!("Playing macro from register '{}'", register);
        Ok(())
    }

    /// Play a macro with a count prefix.
    pub(crate) fn play_macro_with_count(
        &mut self,
        register: char,
        count: usize,
    ) -> Result<(), String> {
        for _ in 0..count {
            self.play_macro(register)?;
        }
        Ok(())
    }

    /// Takes and consumes the count, returning it (or 1 if no count was entered).
    /// This clears the count field.
    #[allow(dead_code)] // Will be used in Step 2 for motion commands
    pub fn take_count(&mut self) -> usize {
        let op_count = self.operator_count.take().unwrap_or(1);
        let motion_count = self.count.take().unwrap_or(1);
        op_count * motion_count
    }

    /// Peeks at the current count without consuming it. Used for UI display.
    pub fn peek_count(&self) -> Option<usize> {
        self.count
    }

    /// Yank the current line into the active register (linewise).
    #[allow(dead_code)]
    pub(crate) fn yank_current_line(&mut self) {
        let line = self.view().cursor.line;
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);
        let content: String = self
            .buffer()
            .content
            .slice(line_start..line_start + line_len)
            .chars()
            .collect();

        // Ensure linewise content ends with newline
        let content = if content.ends_with('\n') {
            content
        } else {
            format!("{}\n", content)
        };

        let reg = self.active_register();
        self.set_register(reg, content, true);
        self.clear_selected_register();
        self.message = "1 line yanked".to_string();
    }

    /// Replace count characters with the replacement character
    pub(crate) fn replace_chars(&mut self, replacement: char, count: usize, changed: &mut bool) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let char_idx = self.buffer().line_to_char(line) + col;

        // Calculate how many chars we can replace on this line (not crossing newline)
        let line_end = self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
        let available = line_end.saturating_sub(char_idx);

        // Don't count the newline character at the end of line
        let line_content = self.buffer().content.line(line);
        let available = if line_content.chars().last() == Some('\n') {
            available.saturating_sub(1)
        } else {
            available
        };

        let to_replace = count.min(available);

        if to_replace > 0 && char_idx < self.buffer().len_chars() {
            // Build the replacement string
            let replacement_str: String = std::iter::repeat_n(replacement, to_replace).collect();

            // Delete the old characters and insert the new ones
            self.delete_with_undo(char_idx, char_idx + to_replace);
            self.insert_with_undo(char_idx, &replacement_str);

            // Keep cursor at the start position (Vim behavior)
            self.view_mut().cursor.col = col;
            self.clamp_cursor_col();
            *changed = true;
        }
    }

    /// Yank count lines starting from current line
    pub(crate) fn yank_lines(&mut self, count: usize) {
        let start_line = self.view().cursor.line;
        let total_lines = self.buffer().len_lines();
        let end_line = (start_line + count).min(total_lines);
        let actual_count = end_line - start_line;

        if actual_count == 0 {
            return;
        }

        let start_char = self.buffer().line_to_char(start_line);
        let end_char = if end_line < total_lines {
            self.buffer().line_to_char(end_line)
        } else {
            self.buffer().len_chars()
        };

        let content: String = self
            .buffer()
            .content
            .slice(start_char..end_char)
            .chars()
            .collect();

        // Ensure linewise content ends with newline
        let content = if content.ends_with('\n') {
            content
        } else {
            format!("{}\n", content)
        };

        let reg = self.active_register();
        self.set_yank_register(reg, content, true);
        self.clear_selected_register();

        // Record highlight region for brief visual flash
        let hl_end_line = end_line.saturating_sub(1).max(start_line);
        self.record_yank_highlight(
            Cursor {
                line: start_line,
                col: 0,
            },
            Cursor {
                line: hl_end_line,
                col: 0,
            },
            true,
        );

        let msg = if actual_count == 1 {
            "1 line yanked".to_string()
        } else {
            format!("{} lines yanked", actual_count)
        };
        self.message = msg;
    }

    /// Paste after cursor (p). Linewise pastes below current line.
    pub fn paste_after(&mut self, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register_content(reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                return;
            }
        };

        self.start_undo_group();

        if is_linewise {
            // Paste below current line
            let line = self.view().cursor.line;
            let line_end = self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
            // If current line doesn't end with newline, we need to add one
            let line_content = self.buffer().content.line(line);
            if line_content.chars().last() == Some('\n') {
                self.insert_with_undo(line_end, &content);
            } else {
                // Insert newline + content
                let content_with_newline = format!("\n{}", content);
                self.insert_with_undo(line_end, &content_with_newline);
            };
            // Move cursor to first non-blank of new line
            self.view_mut().cursor.line += 1;
            self.view_mut().cursor.col = 0;
        } else {
            // Paste after cursor position
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            let char_idx = self.buffer().line_to_char(line) + col;
            // Insert after current char (if line not empty)
            let insert_pos = if self.buffer().line_len_chars(line) > 0 {
                char_idx + 1
            } else {
                char_idx
            };
            self.insert_with_undo(insert_pos, &content);
            // Move cursor to end of pasted text (last char)
            let paste_len = content.chars().count();
            if paste_len > 0 {
                self.view_mut().cursor.col = col + paste_len;
            }
        }

        self.finish_undo_group();
        self.clear_selected_register();
        *changed = true;
    }

    /// Paste before cursor (P). Linewise pastes above current line.
    pub(crate) fn paste_before(&mut self, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register_content(reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                return;
            }
        };

        self.start_undo_group();

        if is_linewise {
            // Paste above current line
            let line = self.view().cursor.line;
            let line_start = self.buffer().line_to_char(line);
            self.insert_with_undo(line_start, &content);
            // Cursor stays on same line number (which is now the pasted line)
            self.view_mut().cursor.col = 0;
        } else {
            // Paste before cursor position
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            let char_idx = self.buffer().line_to_char(line) + col;
            self.insert_with_undo(char_idx, &content);
            // Cursor moves to end of pasted text
            let paste_len = content.chars().count();
            if paste_len > 0 {
                self.view_mut().cursor.col = col + paste_len - 1;
            }
        }

        self.finish_undo_group();
        self.clear_selected_register();
        *changed = true;
    }

    /// Paste after cursor, leave cursor after pasted text (gp).
    pub(crate) fn paste_after_cursor_after(&mut self, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register_content(reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                return;
            }
        };

        self.start_undo_group();

        if is_linewise {
            let line = self.view().cursor.line;
            let line_end = self.buffer().line_to_char(line) + self.buffer().line_len_chars(line);
            let line_content = self.buffer().content.line(line);
            if line_content.chars().last() == Some('\n') {
                self.insert_with_undo(line_end, &content);
            } else {
                let content_with_newline = format!("\n{}", content);
                self.insert_with_undo(line_end, &content_with_newline);
            }
            // Count lines in pasted content to position cursor after
            let pasted_lines = content.chars().filter(|c| *c == '\n').count();
            self.view_mut().cursor.line = line + 1 + pasted_lines;
            let max_line = self.buffer().len_lines().saturating_sub(1);
            if self.view().cursor.line > max_line {
                self.view_mut().cursor.line = max_line;
            }
            self.view_mut().cursor.col = 0;
        } else {
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            let char_idx = self.buffer().line_to_char(line) + col;
            let insert_pos = if self.buffer().line_len_chars(line) > 0 {
                char_idx + 1
            } else {
                char_idx
            };
            self.insert_with_undo(insert_pos, &content);
            // Position cursor after pasted text
            let paste_len = content.chars().count();
            if paste_len > 0 {
                let end_pos = insert_pos + paste_len;
                let new_line = self
                    .buffer()
                    .content
                    .char_to_line(end_pos.min(self.buffer().len_chars().saturating_sub(1)));
                let new_col =
                    end_pos.min(self.buffer().len_chars()) - self.buffer().line_to_char(new_line);
                self.view_mut().cursor.line = new_line;
                self.view_mut().cursor.col = new_col;
            }
        }

        self.finish_undo_group();
        self.clear_selected_register();
        *changed = true;
    }

    /// Paste before cursor, leave cursor after pasted text (gP).
    pub(crate) fn paste_before_cursor_after(&mut self, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register_content(reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                return;
            }
        };

        self.start_undo_group();

        if is_linewise {
            let line = self.view().cursor.line;
            let line_start = self.buffer().line_to_char(line);
            self.insert_with_undo(line_start, &content);
            // Count lines in pasted content to position cursor after
            let pasted_lines = content.chars().filter(|c| *c == '\n').count();
            self.view_mut().cursor.line = line + pasted_lines;
            let max_line = self.buffer().len_lines().saturating_sub(1);
            if self.view().cursor.line > max_line {
                self.view_mut().cursor.line = max_line;
            }
            self.view_mut().cursor.col = 0;
        } else {
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            let char_idx = self.buffer().line_to_char(line) + col;
            self.insert_with_undo(char_idx, &content);
            let paste_len = content.chars().count();
            if paste_len > 0 {
                let end_pos = char_idx + paste_len;
                let new_line = self
                    .buffer()
                    .content
                    .char_to_line(end_pos.min(self.buffer().len_chars().saturating_sub(1)));
                let new_col =
                    end_pos.min(self.buffer().len_chars()) - self.buffer().line_to_char(new_line);
                self.view_mut().cursor.line = new_line;
                self.view_mut().cursor.col = new_col;
            }
        }

        self.finish_undo_group();
        self.clear_selected_register();
        *changed = true;
    }

    /// Replace all characters in visual selection with a single character (visual r{char}).
    pub(crate) fn replace_visual_selection(&mut self, replacement: char, changed: &mut bool) {
        if let Some((start, end)) = self.get_visual_selection_range() {
            self.start_undo_group();
            match self.mode {
                Mode::VisualBlock => {
                    // Block mode: replace each character in the rectangle
                    let start_col = start.col.min(end.col);
                    let end_col = start.col.max(end.col);
                    for line in start.line..=end.line {
                        if line >= self.buffer().len_lines() {
                            break;
                        }
                        let line_start = self.buffer().line_to_char(line);
                        let line_len = self.buffer().line_len_chars(line);
                        let has_nl = self.buffer().content.line(line).chars().last() == Some('\n');
                        let max_col = if has_nl {
                            line_len.saturating_sub(1)
                        } else {
                            line_len
                        };
                        let col_start = start_col.min(max_col);
                        let col_end = (end_col + 1).min(max_col);
                        for col in (col_start..col_end).rev() {
                            let pos = line_start + col;
                            let ch = self.buffer().content.char(pos);
                            if ch != '\n' {
                                self.delete_with_undo(pos, pos + 1);
                                self.insert_with_undo(pos, &replacement.to_string());
                            }
                        }
                    }
                }
                Mode::VisualLine => {
                    // Line mode: replace all non-newline chars on selected lines
                    for line in (start.line..=end.line).rev() {
                        if line >= self.buffer().len_lines() {
                            continue;
                        }
                        let line_start = self.buffer().line_to_char(line);
                        let line_len = self.buffer().line_len_chars(line);
                        let has_nl = self.buffer().content.line(line).chars().last() == Some('\n');
                        let max_col = if has_nl {
                            line_len.saturating_sub(1)
                        } else {
                            line_len
                        };
                        for col in (0..max_col).rev() {
                            let pos = line_start + col;
                            self.delete_with_undo(pos, pos + 1);
                            self.insert_with_undo(pos, &replacement.to_string());
                        }
                    }
                }
                _ => {
                    // Character-wise: replace from start to end (inclusive)
                    let start_pos = self.buffer().line_to_char(start.line) + start.col;
                    let end_pos = self.buffer().line_to_char(end.line) + end.col;
                    for pos in (start_pos..=end_pos).rev() {
                        if pos < self.buffer().len_chars() {
                            let ch = self.buffer().content.char(pos);
                            if ch != '\n' {
                                self.delete_with_undo(pos, pos + 1);
                                self.insert_with_undo(pos, &replacement.to_string());
                            }
                        }
                    }
                }
            }
            self.finish_undo_group();
            self.view_mut().cursor = start;
            self.mode = Mode::Normal;
            self.visual_anchor = None;
            *changed = true;
        }
    }
}

// ─── Additional methods (extracted from mod.rs) ─────────────────────────

impl Engine {
    // =======================================================================
    // Bracket navigation ([ and ] commands)
    // =======================================================================

    /// Jump to next section start (]] or next section end (][).
    /// `end_section`: false = start ('{' in column 0), true = end ('}' in column 0).
    /// In LaTeX buffers: ]] jumps to next \section/\chapter/\subsection/\subsubsection,
    /// ][ jumps to next \end{...}.
    pub(crate) fn jump_section_forward(&mut self, end_section: bool) {
        if self.is_latex_buffer() {
            self.jump_latex_section_forward(end_section);
            return;
        }
        let target_char = if end_section { '}' } else { '{' };
        let total = self.buffer().len_lines();
        let start = self.view().cursor.line + 1;
        for line in start..total {
            let line_start = self.buffer().line_to_char(line);
            if self.buffer().line_len_chars(line) > 0
                && self.buffer().content.char(line_start) == target_char
            {
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = 0;
                return;
            }
        }
    }

    /// Jump to previous section start ([[) or previous section end (][]).
    /// In LaTeX buffers: [[ jumps to previous \section/etc., [] jumps to previous \end{}.
    pub(crate) fn jump_section_backward(&mut self, end_section: bool) {
        if self.is_latex_buffer() {
            self.jump_latex_section_backward(end_section);
            return;
        }
        let target_char = if end_section { '}' } else { '{' };
        let cur = self.view().cursor.line;
        for line in (0..cur).rev() {
            let line_start = self.buffer().line_to_char(line);
            if self.buffer().line_len_chars(line) > 0
                && self.buffer().content.char(line_start) == target_char
            {
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = 0;
                return;
            }
        }
    }

    /// Jump to next method start (]m) — finds next '{' that starts a block.
    /// In LaTeX buffers: ]m jumps to next \begin{...}.
    pub(crate) fn jump_method_start_forward(&mut self) {
        if self.is_latex_buffer() {
            self.jump_latex_env_forward(false);
            return;
        }
        let total_chars = self.buffer().len_chars();
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        let mut pos = cur_pos + 1;
        while pos < total_chars {
            if self.buffer().content.char(pos) == '{' {
                let line = self.buffer().content.char_to_line(pos);
                let line_start = self.buffer().line_to_char(line);
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = pos - line_start;
                return;
            }
            pos += 1;
        }
    }

    /// Jump to previous method start ([m).
    /// In LaTeX buffers: [m jumps to previous \begin{...}.
    pub(crate) fn jump_method_start_backward(&mut self) {
        if self.is_latex_buffer() {
            self.jump_latex_env_backward(false);
            return;
        }
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        if cur_pos == 0 {
            return;
        }
        let mut pos = cur_pos - 1;
        loop {
            if self.buffer().content.char(pos) == '{' {
                let line = self.buffer().content.char_to_line(pos);
                let line_start = self.buffer().line_to_char(line);
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = pos - line_start;
                return;
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
        }
    }

    /// Jump to next method end (]M) — finds next '}'.
    /// In LaTeX buffers: ]M jumps to next \end{...}.
    pub(crate) fn jump_method_end_forward(&mut self) {
        if self.is_latex_buffer() {
            self.jump_latex_env_forward(true);
            return;
        }
        let total_chars = self.buffer().len_chars();
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        let mut pos = cur_pos + 1;
        while pos < total_chars {
            if self.buffer().content.char(pos) == '}' {
                let line = self.buffer().content.char_to_line(pos);
                let line_start = self.buffer().line_to_char(line);
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = pos - line_start;
                return;
            }
            pos += 1;
        }
    }

    /// Jump to previous method end ([M).
    /// In LaTeX buffers: [M jumps to previous \end{...}.
    pub(crate) fn jump_method_end_backward(&mut self) {
        if self.is_latex_buffer() {
            self.jump_latex_env_backward(true);
            return;
        }
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        if cur_pos == 0 {
            return;
        }
        let mut pos = cur_pos - 1;
        loop {
            if self.buffer().content.char(pos) == '}' {
                let line = self.buffer().content.char_to_line(pos);
                let line_start = self.buffer().line_to_char(line);
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = pos - line_start;
                return;
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
        }
    }

    // --- LaTeX-specific motion helpers ---

    /// LaTeX section commands to match for ]] / [[ jumps.
    const LATEX_SECTION_COMMANDS: &'static [&'static str] = &[
        "\\part",
        "\\chapter",
        "\\section",
        "\\subsection",
        "\\subsubsection",
        "\\paragraph",
        "\\subparagraph",
    ];

    /// Jump forward to next LaTeX section command (]]) or \end{} (][).
    pub(crate) fn jump_latex_section_forward(&mut self, end_section: bool) {
        let total = self.buffer().len_lines();
        let start = self.view().cursor.line + 1;
        for line in start..total {
            let line_text = self.buffer().content.line(line).chars().collect::<String>();
            let trimmed = line_text.trim_start();
            if end_section {
                if trimmed.starts_with("\\end{") {
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = 0;
                    return;
                }
            } else {
                for cmd in Self::LATEX_SECTION_COMMANDS {
                    if let Some(after) = trimmed.strip_prefix(cmd) {
                        if after.starts_with('{') || after.starts_with('*') || after.is_empty() {
                            self.view_mut().cursor.line = line;
                            self.view_mut().cursor.col = 0;
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Jump backward to previous LaTeX section command ([[) or \end{} ([]).
    pub(crate) fn jump_latex_section_backward(&mut self, end_section: bool) {
        let cur = self.view().cursor.line;
        for line in (0..cur).rev() {
            let line_text = self.buffer().content.line(line).chars().collect::<String>();
            let trimmed = line_text.trim_start();
            if end_section {
                if trimmed.starts_with("\\end{") {
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = 0;
                    return;
                }
            } else {
                for cmd in Self::LATEX_SECTION_COMMANDS {
                    if let Some(after) = trimmed.strip_prefix(cmd) {
                        if after.starts_with('{') || after.starts_with('*') || after.is_empty() {
                            self.view_mut().cursor.line = line;
                            self.view_mut().cursor.col = 0;
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Jump forward to next \begin{} (is_end=false) or \end{} (is_end=true).
    pub(crate) fn jump_latex_env_forward(&mut self, is_end: bool) {
        let needle = if is_end { "\\end{" } else { "\\begin{" };
        let total = self.buffer().len_lines();
        let start_line = self.view().cursor.line;
        let start_col = self.view().cursor.col;
        for line in start_line..total {
            let line_text = self.buffer().content.line(line).chars().collect::<String>();
            let search_from = if line == start_line { start_col + 1 } else { 0 };
            if search_from < line_text.len() {
                if let Some(rel) = line_text[search_from..].find(needle) {
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = search_from + rel;
                    return;
                }
            }
        }
    }

    /// Jump backward to previous \begin{} (is_end=false) or \end{} (is_end=true).
    pub(crate) fn jump_latex_env_backward(&mut self, is_end: bool) {
        let needle = if is_end { "\\end{" } else { "\\begin{" };
        let start_line = self.view().cursor.line;
        let start_col = self.view().cursor.col;
        for line in (0..=start_line).rev() {
            let line_text = self.buffer().content.line(line).chars().collect::<String>();
            let search_end = if line == start_line {
                start_col
            } else {
                line_text.len()
            };
            if let Some(pos) = line_text[..search_end].rfind(needle) {
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = pos;
                return;
            }
        }
    }

    /// Jump forward to next unmatched close bracket (]} or ])).
    pub(crate) fn jump_unmatched_forward(&mut self, open: char, close: char) {
        let total_chars = self.buffer().len_chars();
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        let mut pos = cur_pos + 1;
        let mut depth: i32 = 0;
        while pos < total_chars {
            let ch = self.buffer().content.char(pos);
            if ch == open {
                depth += 1;
            } else if ch == close {
                if depth == 0 {
                    let line = self.buffer().content.char_to_line(pos);
                    let line_start = self.buffer().line_to_char(line);
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = pos - line_start;
                    return;
                }
                depth -= 1;
            }
            pos += 1;
        }
    }

    /// Jump backward to previous unmatched open bracket ([{ or [().
    pub(crate) fn jump_unmatched_backward(&mut self, open: char, close: char) {
        let cur_pos = self.buffer().line_to_char(self.view().cursor.line) + self.view().cursor.col;
        if cur_pos == 0 {
            return;
        }
        let mut pos = cur_pos - 1;
        let mut depth: i32 = 0;
        loop {
            let ch = self.buffer().content.char(pos);
            if ch == close {
                depth += 1;
            } else if ch == open {
                if depth == 0 {
                    let line = self.buffer().content.char_to_line(pos);
                    let line_start = self.buffer().line_to_char(line);
                    self.view_mut().cursor.line = line;
                    self.view_mut().cursor.col = pos - line_start;
                    return;
                }
                depth -= 1;
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
        }
    }

    // =======================================================================
    // Toggle case (~)
    // =======================================================================

    /// Toggle the case of `count` characters starting at the cursor, advance cursor.
    pub(crate) fn toggle_case_at_cursor(&mut self, count: usize, changed: &mut bool) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let char_idx = self.buffer().line_to_char(line) + col;

        // How many chars are available on this line (excluding trailing newline)?
        let line_len = self.buffer().line_len_chars(line);
        let line_content = self.buffer().content.line(line);
        let available = if line_content.chars().last() == Some('\n') {
            line_len.saturating_sub(1)
        } else {
            line_len
        };
        let remaining = available.saturating_sub(col);
        let to_toggle = count.min(remaining);

        if to_toggle == 0 {
            return;
        }

        // Read chars to toggle
        let chars: Vec<char> = self
            .buffer()
            .content
            .slice(char_idx..char_idx + to_toggle)
            .chars()
            .collect();

        // Build replacement: toggle case of each char
        let toggled: String = chars
            .iter()
            .map(|&c| {
                if c.is_uppercase() {
                    c.to_lowercase().next().unwrap_or(c)
                } else if c.is_lowercase() {
                    c.to_uppercase().next().unwrap_or(c)
                } else {
                    c
                }
            })
            .collect();

        self.start_undo_group();
        self.delete_with_undo(char_idx, char_idx + to_toggle);
        self.insert_with_undo(char_idx, &toggled);
        self.finish_undo_group();

        // Advance cursor by number of chars toggled (clamped to line end)
        let new_col = (col + to_toggle).min(available.saturating_sub(1));
        self.view_mut().cursor.col = new_col;
        self.clamp_cursor_col();
        *changed = true;
    }

    // =======================================================================
    // Join lines (J)
    // =======================================================================

    /// Join `count` lines starting at cursor. Collapses the newline + leading
    /// whitespace of the next line into a single space (no space before `)`).
    pub(crate) fn join_lines(&mut self, count: usize, changed: &mut bool) {
        let total_lines = self.buffer().len_lines();
        let start_line = self.view().cursor.line;

        // We join (count) times; each join merges current line with next
        let joins = count.min(total_lines.saturating_sub(start_line + 1));
        if joins == 0 {
            return;
        }

        self.start_undo_group();
        for _ in 0..joins {
            let cur_line = self.view().cursor.line;
            let next_line = cur_line + 1;
            if next_line >= self.buffer().len_lines() {
                break;
            }

            // Find position of newline at end of current line
            let cur_line_len = self.buffer().line_len_chars(cur_line);
            let cur_line_start = self.buffer().line_to_char(cur_line);
            // The newline is the last char of the current line
            let newline_pos = cur_line_start + cur_line_len - 1;

            // Count leading whitespace on next line
            let next_line_start = self.buffer().line_to_char(next_line);
            let next_line_content: String = self.buffer().content.line(next_line).chars().collect();
            let leading_ws = next_line_content
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .count();

            // Determine what char comes after the whitespace on the next line
            let next_non_ws = next_line_content.chars().nth(leading_ws);

            // Delete: newline + leading whitespace of next line
            let del_end = next_line_start + leading_ws;
            self.delete_with_undo(newline_pos, del_end);

            // Insert a space unless the next non-ws char is ')' or next line was empty/only ws
            // Also don't add space if the current line ends with a space
            let should_add_space = !matches!(next_non_ws, None | Some(')') | Some(']') | Some('}'));
            // Check if current line ends with space (after the newline was removed)
            let cur_end_char =
                self.buffer().line_to_char(cur_line) + self.buffer().line_len_chars(cur_line);
            let ends_with_space = cur_end_char > self.buffer().line_to_char(cur_line)
                && self.buffer().content.char(cur_end_char - 1) == ' ';

            if should_add_space && !ends_with_space {
                self.insert_with_undo(newline_pos, " ");
            }
        }
        self.finish_undo_group();

        // Cursor stays at start of original line
        self.clamp_cursor_col();
        *changed = true;
    }

    // =======================================================================
    // Scroll cursor to position (zz / zt / zb)
    // =======================================================================

    /// Scroll so that cursor line is centered in viewport.
    pub(crate) fn scroll_cursor_center(&mut self) {
        let cursor_line = self.view().cursor.line;
        let half = self.viewport_lines() / 2;
        let new_top = cursor_line.saturating_sub(half);
        self.view_mut().scroll_top = new_top;
    }

    /// Scroll so that cursor line is at the top of viewport.
    pub(crate) fn scroll_cursor_top(&mut self) {
        let cursor_line = self.view().cursor.line;
        self.view_mut().scroll_top = cursor_line;
    }

    /// Scroll so that cursor line is at the bottom of viewport.
    pub(crate) fn scroll_cursor_bottom(&mut self) {
        let cursor_line = self.view().cursor.line;
        let viewport = self.viewport_lines();
        let new_top = cursor_line.saturating_sub(viewport.saturating_sub(1));
        self.view_mut().scroll_top = new_top;
    }

    // =======================================================================
    // Jump list (Ctrl-O / Ctrl-I)
    // =======================================================================

    /// Push (line, col) to the change list, capped at 100 entries.
    pub(crate) fn push_change_location(&mut self, line: usize, col: usize) {
        // Truncate any forward entries (if we navigated back with g;)
        self.change_list.truncate(self.change_list_pos);
        // Avoid duplicate consecutive entries
        if self.change_list.last() == Some(&(line, col)) {
            return;
        }
        self.change_list.push((line, col));
        if self.change_list.len() > 100 {
            self.change_list.remove(0);
        }
        self.change_list_pos = self.change_list.len();
    }

    /// Push the current cursor position onto the jump list.
    pub fn push_jump_location(&mut self) {
        // Save pre-jump position for '' / `` marks
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        self.last_jump_pos = Some((line, col));

        let file = self.active_buffer_state().file_path.clone();
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;

        // Truncate forward history when a new jump is made
        if self.jump_list_pos < self.jump_list.len() {
            self.jump_list.truncate(self.jump_list_pos);
        }

        // Don't push a duplicate of the current top entry
        if let Some(last) = self.jump_list.last() {
            if last.0 == file && last.1 == line && last.2 == col {
                return;
            }
        }

        self.jump_list.push((file, line, col));

        // Cap at 100 entries
        if self.jump_list.len() > 100 {
            self.jump_list.remove(0);
        }

        self.jump_list_pos = self.jump_list.len();
    }

    /// Navigate backward in the jump list (Ctrl-O).
    pub fn jump_list_back(&mut self) {
        // When at the "live" end (not stored in list), save current position
        // so Ctrl-I can return to it, then jump to the previous entry.
        if self.jump_list_pos == self.jump_list.len() {
            if self.jump_list.is_empty() {
                self.message = "Already at oldest position in jump list".to_string();
                return;
            }
            let file = self.active_buffer_state().file_path.clone();
            let line = self.view().cursor.line;
            let col = self.view().cursor.col;
            #[allow(clippy::unnecessary_map_or)] // is_none_or requires Rust 1.82+
            let should_push = self.jump_list.last().map_or(true, |last| {
                last.0 != file || last.1 != line || last.2 != col
            });
            if should_push {
                self.jump_list.push((file, line, col));
                if self.jump_list.len() > 100 {
                    self.jump_list.remove(0);
                }
            }
            // Jump to the entry BEFORE the one we just saved
            // (list.len()-1 is current, list.len()-2 is the previous)
            if self.jump_list.len() < 2 {
                self.message = "Already at oldest position in jump list".to_string();
                return;
            }
            self.jump_list_pos = self.jump_list.len() - 2;
            self.apply_jump_list_entry(self.jump_list_pos);
            return;
        }

        // We're inside the list — go to the previous entry
        if self.jump_list_pos == 0 {
            self.message = "Already at oldest position in jump list".to_string();
            return;
        }

        self.jump_list_pos -= 1;
        self.apply_jump_list_entry(self.jump_list_pos);
    }

    /// Navigate forward in the jump list (Ctrl-I / Tab).
    pub fn jump_list_forward(&mut self) {
        if self.jump_list_pos + 1 >= self.jump_list.len() {
            self.message = "Already at newest position in jump list".to_string();
            return;
        }

        self.jump_list_pos += 1;
        self.apply_jump_list_entry(self.jump_list_pos);
    }

    /// Move to the position stored at the given jump list index.
    pub(crate) fn apply_jump_list_entry(&mut self, idx: usize) {
        let entry = match self.jump_list.get(idx) {
            Some(e) => e.clone(),
            None => return,
        };

        let (file, line, col) = entry;

        // If cross-file, open the file
        let current_file = self.active_buffer_state().file_path.clone();
        if file != current_file {
            if let Some(path) = &file {
                let path = path.clone();
                let _ = self.open_file_with_mode(&path, OpenMode::Permanent);
            }
        }

        let max_line = self.buffer().len_lines().saturating_sub(1);
        self.view_mut().cursor.line = line.min(max_line);
        self.view_mut().cursor.col = col;
        self.clamp_cursor_col();
    }

    // =======================================================================
    // Indent / Dedent (>> / <<)
    // =======================================================================

    /// Indent `count` lines starting at `start_line` by shift_width.
    pub(crate) fn indent_lines(&mut self, start_line: usize, count: usize, changed: &mut bool) {
        let indent_str = if self.settings.expand_tab {
            " ".repeat(self.effective_shift_width())
        } else {
            "\t".to_string()
        };

        self.start_undo_group();
        let total = self.buffer().len_lines();
        for i in 0..count {
            let line_idx = start_line + i;
            if line_idx >= total {
                break;
            }
            let line_start = self.buffer().line_to_char(line_idx);
            self.insert_with_undo(line_start, &indent_str);
        }
        self.finish_undo_group();
        *changed = true;
    }

    /// Dedent `count` lines starting at `start_line`.
    /// Removes up to shift_width columns, but caps removal at the minimum
    /// indent across all non-blank lines in the selection to preserve
    /// relative nesting structure.
    pub(crate) fn dedent_lines(&mut self, start_line: usize, count: usize, changed: &mut bool) {
        let sw = self.effective_shift_width();
        let total = self.buffer().len_lines();

        // First pass: find minimum leading whitespace (visual columns) across
        // all non-blank lines in the selection.
        let mut min_indent = usize::MAX;
        for i in 0..count {
            let line_idx = start_line + i;
            if line_idx >= total {
                break;
            }
            let line_content: String = self.buffer().content.line(line_idx).chars().collect();
            let trimmed = line_content.trim_end_matches(['\n', '\r']);
            // Skip blank/whitespace-only lines — they shouldn't constrain removal
            if trimmed.trim().is_empty() {
                continue;
            }
            let mut visual_indent = 0;
            for ch in trimmed.chars() {
                match ch {
                    ' ' => visual_indent += 1,
                    '\t' => visual_indent += sw - (visual_indent % sw),
                    _ => break,
                }
            }
            min_indent = min_indent.min(visual_indent);
        }

        if min_indent == usize::MAX || min_indent == 0 {
            return;
        }

        // Remove at most shift_width, but never more than the least-indented
        // non-blank line has — this preserves relative nesting.
        let remove_cols = sw.min(min_indent);

        self.start_undo_group();
        // Work backwards to avoid invalidating char positions
        for i in (0..count).rev() {
            let line_idx = start_line + i;
            if line_idx >= total {
                continue;
            }
            let line_start = self.buffer().line_to_char(line_idx);
            let line_content: String = self.buffer().content.line(line_idx).chars().collect();
            let mut removed_visual = 0;
            let mut removed_chars = 0;
            for ch in line_content.chars() {
                if removed_visual >= remove_cols {
                    break;
                }
                match ch {
                    ' ' => {
                        removed_visual += 1;
                        removed_chars += 1;
                    }
                    '\t' => {
                        let tab_width = sw - (removed_visual % sw);
                        if removed_visual + tab_width > remove_cols {
                            break; // don't partially remove a tab
                        }
                        removed_visual += tab_width;
                        removed_chars += 1;
                    }
                    _ => break,
                }
            }
            if removed_chars > 0 {
                self.delete_with_undo(line_start, line_start + removed_chars);
            }
        }
        self.finish_undo_group();
        if count > 0 {
            *changed = true;
        }
    }
}
