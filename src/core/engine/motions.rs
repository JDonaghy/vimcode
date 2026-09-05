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

        pos = self.skip_separators_forward_word_aware(pos);

        if pos >= total_chars {
            pos = total_chars.saturating_sub(1);
        }

        let new_line = self.buffer().content.char_to_line(pos);
        let line_start = self.buffer().line_to_char(new_line);
        self.view_mut().cursor.line = new_line;
        self.view_mut().cursor.col = pos - line_start;
    }

    /// Skip a run of separator whitespace forward from `pos`, treating a
    /// completely blank line as a word in its own right (Vim: "An empty
    /// line is also considered to be a word") rather than something to skip
    /// through. Returns the position of the next non-blank word start, or
    /// the position of a blank line's own (only) char if one is crossed, or
    /// `len_chars()` if the buffer ends first.
    fn skip_separators_forward_word_aware(&self, mut pos: usize) -> usize {
        let total = self.buffer().len_chars();
        loop {
            if pos >= total {
                return pos;
            }
            let ch = self.buffer().content.char(pos);
            if ch == '\n' {
                let line_of_nl = self.buffer().content.char_to_line(pos);
                let next_line = line_of_nl + 1;
                if next_line >= self.buffer().len_lines() {
                    return total;
                }
                pos = self.buffer().line_to_char(next_line);
                if pos >= total || self.is_line_blank_strict(next_line) {
                    return pos;
                }
            } else if ch.is_whitespace() {
                pos += 1;
            } else {
                return pos;
            }
        }
    }

    /// Mirror of `skip_separators_forward_word_aware` for backward motions
    /// (`b`, `ge`): skip whitespace backward from `pos`, stopping as soon as
    /// landing on a blank line's own char (that blank line is itself the
    /// previous word) or at the start of the buffer.
    fn skip_separators_backward_word_aware(&self, mut pos: usize) -> usize {
        loop {
            let ch = self.buffer().content.char(pos);
            if !ch.is_whitespace() {
                return pos;
            }
            if ch == '\n' {
                let ln = self.buffer().content.char_to_line(pos);
                if self.is_line_blank_strict(ln) {
                    return pos;
                }
            }
            if pos == 0 {
                return 0;
            }
            pos -= 1;
        }
    }

    pub(crate) fn move_word_backward(&mut self) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let mut pos = self.buffer().line_to_char(line) + col;

        if pos == 0 {
            return;
        }
        pos -= 1;

        pos = self.skip_separators_backward_word_aware(pos);

        let ch = self.buffer().content.char(pos);
        if ch == '\n' {
            // Landed exactly on a blank line, which Vim counts as its own
            // word — nothing more to extend backward into.
        } else if is_word_char(ch) {
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

        // Step 3: Skip whitespace backward, treating a blank line as a word
        // in its own right (so `ge` can land on one) rather than skipping
        // through it.
        pos = self.skip_separators_backward_word_aware(pos);

        // If we stopped on whitespace that ISN'T a blank line, we ran off
        // the start of the buffer without finding a previous word.
        let ch = self.buffer().content.char(pos);
        if ch.is_whitespace() {
            let ln = self.buffer().content.char_to_line(pos);
            if !self.is_line_blank_strict(ln) {
                return;
            }
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

        // Step back to skip current whitespace / sentence start. A blank
        // line is itself a paragraph/sentence boundary (`word:( para`), so
        // stop there instead of skipping through it.
        pos = pos.saturating_sub(1);
        while pos > 0 && self.buffer().content.char(pos).is_whitespace() {
            if self.buffer().content.char(pos) == '\n'
                && self.is_line_blank_strict(self.buffer().content.char_to_line(pos))
            {
                break;
            }
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

    /// Normal-mode `<C-a>` / `<C-x>`: find the number at or after the cursor on
    /// the current line and add `delta` to it.
    pub(crate) fn increment_number_at_cursor(&mut self, delta: i64, changed: &mut bool) {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        self.start_undo_group();
        let hit = self.addsub_on_line(line, col, delta, None).is_some();
        self.finish_undo_group();
        if hit {
            *changed = true;
        } else {
            self.message = "No number under cursor".to_string();
        }
    }

    /// One `do_addsub()` application on `line`.
    ///
    /// `sel` is `Some(selected_len)` when driven from Visual mode — the search
    /// for a digit is then confined to `selected_len` chars starting at `col`,
    /// and a `-` immediately before the number only counts as a sign when it is
    /// itself inside the selection (Vim: `vl<C-a>` on the `5` of `x -5` yields
    /// `x -6`, not `x -4`).
    ///
    /// Returns the column the replacement started at when the line changed.
    ///
    /// The caller owns the undo group — `start_undo_group()` finishes any group
    /// already open, so a per-line group here would split a Visual-mode
    /// `<C-a>` across N undo steps.
    pub(crate) fn addsub_on_line(
        &mut self,
        line: usize,
        col: usize,
        delta: i64,
        sel: Option<usize>,
    ) -> Option<usize> {
        if line >= self.buffer().len_lines() {
            return None;
        }
        let line_text: String = self.buffer().content.line(line).chars().collect();
        let chars: Vec<char> = line_text.trim_end_matches('\n').chars().collect();
        let (start, len, new_text) = addsub_in_line(&chars, col, delta, NrFormats::default(), sel)?;

        let line_start = self.buffer().line_to_char(line);
        self.delete_with_undo(line_start + start, line_start + start + len);
        self.insert_with_undo(line_start + start, &new_text);

        self.view_mut().cursor.line = line;
        self.view_mut().cursor.col = start + new_text.chars().count().saturating_sub(1);
        Some(start)
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
        let joins = if count <= 1 { 1 } else { count - 1 };
        let joins = joins.min(total_lines.saturating_sub(start_line + 1));
        if joins == 0 {
            return;
        }

        self.start_undo_group();
        let mut join_col = 0usize;
        for _ in 0..joins {
            let cur_line = self.view().cursor.line;
            let next_line = cur_line + 1;
            if next_line >= self.buffer().len_lines() {
                break;
            }

            let cur_line_len = self.buffer().line_len_chars(cur_line);
            let cur_line_start = self.buffer().line_to_char(cur_line);
            let newline_pos = cur_line_start + cur_line_len - 1;
            join_col = newline_pos - cur_line_start;

            // gJ does NOT strip leading whitespace — just remove the newline
            self.delete_with_undo(newline_pos, newline_pos + 1);
        }
        self.finish_undo_group();

        self.view_mut().cursor.col = join_col;
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

    /// Like `file_path_under_cursor`, but also parses a trailing `:<line>`
    /// suffix (e.g. `src/main.rs:42`, `foo.txt:10:3`). Returns the resolved
    /// path and an optional 1-based line number.
    pub(crate) fn file_path_and_line_under_cursor(
        &self,
    ) -> Option<(std::path::PathBuf, Option<usize>)> {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;

        let line_text: String = self.buffer().content.line(line).chars().collect();
        let chars: Vec<char> = line_text.chars().collect();

        // Path chars: same as file_path_under_cursor but ALLOW ':' so we capture the suffix
        let is_path_or_colon =
            |c: char| !c.is_whitespace() && c != '"' && c != '\'' && c != ',' && c != ';';

        let mut start = col;
        let mut end = col;

        while start > 0 && is_path_or_colon(chars[start - 1]) {
            start -= 1;
        }
        while end < chars.len() && is_path_or_colon(chars[end]) {
            end += 1;
        }
        while end > start && (chars[end - 1] == '\n' || chars[end - 1] == '\r') {
            end -= 1;
        }
        // Strip trailing colon (e.g. "foo.rs:" at end of sentence)
        while end > start && chars[end - 1] == ':' {
            end -= 1;
        }

        if start >= end {
            return None;
        }

        let token: String = chars[start..end].iter().collect();
        if token.is_empty() {
            return None;
        }

        // Split off `:line` or `:line:col` suffix — try progressively stripping
        // colon-delimited numeric suffixes to find a valid file path.
        let mut path_part = token.as_str();
        let mut line_num: Option<usize> = None;

        // Try stripping `:col` then `:line` (handles `path:line:col`)
        for _ in 0..2 {
            if let Some(colon_pos) = path_part.rfind(':') {
                let suffix = &path_part[colon_pos + 1..];
                if let Ok(n) = suffix.parse::<usize>() {
                    // Remember the first (outermost) number stripped as line, but
                    // on the second pass it becomes the actual line number.
                    line_num = Some(n);
                    path_part = &path_part[..colon_pos];
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if path_part.is_empty() {
            return None;
        }

        let path = std::path::PathBuf::from(path_part);

        // Resolve path: workspace root, then current file's dir
        let resolved = if path.is_absolute() {
            if path.exists() {
                Some(path)
            } else {
                None
            }
        } else {
            let mut found = None;
            if let Some(ref root) = self.workspace_root {
                let abs = root.join(&path);
                if abs.exists() {
                    found = Some(abs);
                }
            }
            if found.is_none() {
                if let Some(file_path) = self.active_buffer_state().file_path.as_ref() {
                    if let Some(dir) = file_path.parent() {
                        let abs = dir.join(&path);
                        if abs.exists() {
                            found = Some(abs);
                        }
                    }
                }
            }
            found
        };

        resolved.map(|p| (p, line_num))
    }

    // --- g* / g#: partial word search ---

    pub(crate) fn search_word_under_cursor_partial(&mut self, forward: bool) {
        self.search_word_under_cursor_generic(forward, false);
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
            self.paste_after(1, changed);
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
            self.paste_before(1, changed);
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
                // Vim steps cursor one left when leaving Replace mode (unless at col 0)
                if self.view().cursor.col > 0 {
                    self.view_mut().cursor.col -= 1;
                }
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
        let max_line = total_lines.saturating_sub(1);
        let mut line = self.view().cursor.line;

        // `}` always moves at least one line, and never moves past the last
        // line of the buffer.
        if line >= max_line {
            return;
        }

        // A run of consecutive blank lines is a single paragraph boundary:
        // if we're starting inside one, skip past the whole run first so a
        // second `}` doesn't stop again one line later within the same run.
        if self.is_line_blank_strict(line) {
            while line < max_line && self.is_line_blank_strict(line) {
                line += 1;
            }
        }
        // Now advance to the next blank line (the next paragraph boundary),
        // or the last line of the buffer if there isn't one.
        while line < max_line && !self.is_line_blank_strict(line) {
            line += 1;
        }

        self.view_mut().cursor.line = line;
        self.view_mut().cursor.col = 0;
    }

    pub(crate) fn move_paragraph_backward(&mut self) {
        let mut line = self.view().cursor.line;

        // `{` always moves at least one line, and never moves past line 0.
        if line == 0 {
            return;
        }

        // Mirror of the forward run-skipping: starting inside a blank-line
        // run skips the whole run before searching for the previous one.
        if self.is_line_blank_strict(line) {
            while line > 0 && self.is_line_blank_strict(line) {
                line -= 1;
            }
        }
        while line > 0 && !self.is_line_blank_strict(line) {
            line -= 1;
        }

        self.view_mut().cursor.line = line;
        self.view_mut().cursor.col = 0;
    }

    /// Returns true if the line has no characters at all (besides its own
    /// line terminator). Unlike `is_line_empty`, a whitespace-only line
    /// (e.g. `"   "`) is NOT considered blank here — this is the stricter
    /// notion Vim's `{`/`}` paragraph motions and blank-line word/sentence
    /// boundaries use (:help paragraph: a set of consecutive blank lines
    /// separates paragraphs, and whitespace-only doesn't count).
    pub(crate) fn is_line_blank_strict(&self, line: usize) -> bool {
        if line >= self.buffer().len_lines() {
            return false;
        }
        let len = self.buffer().line_len_chars(line);
        if len == 0 {
            return true;
        }
        len == 1 && self.buffer().content.line(line).char(0) == '\n'
    }

    /// Returns true if the line is empty or contains only whitespace.
    pub(crate) fn is_line_empty(&self, line: usize) -> bool {
        if line >= self.buffer().len_lines() {
            return false;
        }

        let line_len = self.buffer().line_len_chars(line);

        // A line with zero chars is trivially empty. Do NOT special-case
        // `line_len == 1` as "just a newline" — the *last* line of a buffer
        // with no trailing newline also has length 1, and its one character
        // can be real content (e.g. a single-char last paragraph). That
        // false positive made `ap`/`ip` paragraph-boundary detection treat
        // that line as blank: `dap` would swallow it as a "trailing blank"
        // and delete past it, and `ip`/`ap` would stop just short of
        // including it. The loop below already handles both `line_len == 0`
        // (never runs, falls through to `true`) and a real lone `"\n"`
        // (matches whitespace) correctly, so it covers this case too.
        if line_len == 0 {
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
    pub(crate) fn find_char(&mut self, motion_type: char, target: char) -> bool {
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
                        return true;
                    }
                }
            }
            'F'
                // Find backward (inclusive): search left of cursor
                if col > 0 => {
                    for i in (0..col).rev() {
                        let ch = self.buffer().content.char(line_start + i);
                        if ch == target {
                            self.view_mut().cursor.col = i;
                            return true;
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
                        return true;
                    }
                }
            }
            'T'
                // Till backward (exclusive): stop after target
                if col > 0 => {
                    for i in (0..col).rev() {
                        let ch = self.buffer().content.char(line_start + i);
                        if ch == target {
                            self.view_mut().cursor.col = i + 1;
                            return true;
                        }
                    }
                }
            _ => {}
        }
        // Character not found - cursor doesn't move (Vim behavior)
        false
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
            if actual_motion == 't' || actual_motion == 'T' {
                self.repeat_till(actual_motion, target);
            } else {
                self.find_char(actual_motion, target);
            }
        }
    }

    /// Repeat a `t`/`T` "till" search via `;`/`,` (Vim's default `cpoptions`
    /// behavior). Because `t`/`T` park the cursor one character short of the
    /// target, a naive repeat immediately re-finds that same adjacent target
    /// and "gets stuck" (zero movement). Vim's default `;`/`,` skip that
    /// immediately-adjacent occurrence and advance to the next one instead —
    /// which is always safe to do unconditionally: if the adjacent character
    /// isn't the target, starting one position further finds the exact same
    /// first match a plain search would have.
    fn repeat_till(&mut self, motion_type: char, target: char) -> bool {
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        let line_start = self.buffer().line_to_char(line);
        let line_len = self.buffer().line_len_chars(line);
        match motion_type {
            't' => {
                for i in (col + 2)..line_len {
                    let ch = self.buffer().content.char(line_start + i);
                    if ch == target && ch != '\n' {
                        self.view_mut().cursor.col = i - 1;
                        return true;
                    }
                }
            }
            'T' if col > 1 => {
                for i in (0..col - 1).rev() {
                    let ch = self.buffer().content.char(line_start + i);
                    if ch == target {
                        self.view_mut().cursor.col = i + 1;
                        return true;
                    }
                }
            }
            _ => {}
        }
        false
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

        // Vim's built-in `%` (no matchit plugin) treats a bracket as not a
        // real bracket at all when it falls inside an unterminated
        // double-quoted string earlier on the same line, and does nothing
        // rather than matching across the string boundary.
        if self.quotes_before_on_line(line, char_pos) % 2 == 1 {
            return;
        }

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

    /// Count `"` characters on `line` before `pos` (exclusive). An odd count
    /// means `pos` falls inside an unterminated double-quoted string that
    /// started earlier on the line.
    fn quotes_before_on_line(&self, line: usize, pos: usize) -> usize {
        let line_start = self.buffer().line_to_char(line);
        (line_start..pos)
            .filter(|&i| self.buffer().content.char(i) == '"')
            .count()
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
        // Guard against stale cursors that are out of range for the current
        // buffer (e.g. after :help / :split reassigns a shorter buffer to the
        // active window while the view still holds the previous cursor position).
        if line >= self.buffer().len_lines() {
            self.bracket_match = None;
            return;
        }
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
            '(' | ')' | 'b' => self.find_bracket_object(modifier, '(', ')', cursor_pos),
            '{' | '}' | 'B' => self.find_bracket_object(modifier, '{', '}', cursor_pos),
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

        // `iw` on whitespace selects the contiguous run of blanks itself
        // (Vim treats a blank run as its own "word" class for `iw`/`iW`) —
        // it is NOT "no object", which is what an earlier version of this
        // function returned, silently no-opping `diw`/`ciw` etc. on any
        // whitespace character (including a lone isolated space).
        if modifier == 'i' && char_at_cursor.is_whitespace() && char_at_cursor != '\n' {
            let mut start = cursor_pos;
            let mut end = cursor_pos;
            while start > 0 {
                let ch = self.buffer().content.char(start - 1);
                if !ch.is_whitespace() || ch == '\n' {
                    break;
                }
                start -= 1;
            }
            while end < total_chars {
                let ch = self.buffer().content.char(end);
                if !ch.is_whitespace() || ch == '\n' {
                    break;
                }
                end += 1;
            }
            return if start < end {
                Some((start, end))
            } else {
                None
            };
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

        // For 'aw', include trailing whitespace; if none, include leading whitespace
        if modifier == 'a' {
            let end_before = end;
            while end < total_chars {
                let ch = self.buffer().content.char(end);
                if !ch.is_whitespace() || ch == '\n' {
                    break;
                }
                end += 1;
            }
            // No trailing whitespace consumed — try leading instead
            if end == end_before {
                while start > 0 {
                    let ch = self.buffer().content.char(start - 1);
                    if !ch.is_whitespace() || ch == '\n' {
                        break;
                    }
                    start -= 1;
                }
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

        // Cursor isn't inside/on a bracket pair: Vim still finds `i(`/`a(`
        // etc. by scanning forward on the current line for the next opening
        // bracket, the same fallback `%` uses (`:h ib`; #806, "mac:macro
        // with ci(" — cursor started on `f` before `f(a)`'s paren).
        let open_pos = match open_pos {
            Some(p) => p,
            None => {
                let line = self.buffer().content.char_to_line(cursor_pos);
                let line_start = self.buffer().line_to_char(line);
                let line_len = self.buffer().line_len_chars(line);
                let mut found = None;
                for i in (cursor_pos - line_start)..line_len {
                    let pos = line_start + i;
                    if pos >= total_chars {
                        break;
                    }
                    let ch = self.buffer().content.char(pos);
                    if ch == '\n' {
                        break;
                    }
                    if ch == open_char {
                        found = Some(pos);
                        break;
                    }
                }
                found?
            }
        };

        // Find matching closing bracket
        let close_pos = self.find_matching_bracket(open_pos, open_char, close_char, true)?;

        // Return range based on modifier
        if modifier == 'i' {
            // Inner: exclude brackets
            if open_pos < close_pos {
                let open_line = self.buffer().content.char_to_line(open_pos);
                let close_line = self.buffer().content.char_to_line(close_pos);
                if open_line != close_line {
                    // Multiline: Vim makes inner bracket objects linewise —
                    // delete from start of line after open bracket to start of
                    // line with close bracket.
                    let start = self.buffer().line_to_char(open_line + 1);
                    let end = self.buffer().line_to_char(close_line);
                    if start <= end {
                        Some((start, end))
                    } else {
                        // Empty interior (brackets on adjacent lines)
                        Some((start, start))
                    }
                } else {
                    Some((open_pos + 1, close_pos))
                }
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

        let (mut start_pos, end_pos) = range;

        // `ip`/`ap` are *linewise* objects, so `dip`/`dap` must remove whole
        // lines — including one line separator per deleted line. The range
        // normally carries the deleted lines' trailing newlines, but the final
        // paragraph of a buffer with no trailing newline has none to carry: the
        // separator that has to go is the one *before* it. Without absorbing
        // it, `dip` on the last line of "a\n\nb" leaves "a\n\n" (an extra blank
        // line) with the cursor on a line Vim has already removed (#803).
        if operator == 'd'
            && obj_type == 'p'
            && end_pos == self.buffer().len_chars()
            && start_pos > 0
            && self.buffer().content.char(start_pos - 1) == '\n'
        {
            start_pos -= 1;
        }

        if start_pos >= end_pos {
            // Empty inner range (e.g. ci( on "()"). For 'c' operator,
            // still enter insert mode at the position between the delimiters.
            if operator == 'c' {
                let line = self.buffer().content.char_to_line(start_pos);
                let line_start = self.buffer().line_to_char(line);
                self.view_mut().cursor.line = line;
                self.view_mut().cursor.col = start_pos - line_start;
                self.mode = Mode::Insert;
                self.start_undo_group();
                self.insert_text_buffer.clear();
                *changed = true;
            }
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
            // Deleting to end of buffer: just delete from start of first
            // deleted line to EOF. This preserves the previous line's
            // trailing newline (if any).
            (line_start, line_end)
        };

        self.delete_with_undo(delete_start, delete_end);
        *changed = true;

        let new_num_lines = self.buffer().len_lines();
        if self.view().cursor.line >= new_num_lines && new_num_lines > 0 {
            self.view_mut().cursor.line = new_num_lines - 1;
        }
        // Vim leaves the cursor on the same column it was on (clamped to the
        // resulting line's length) — it does NOT reset to column 0 or to the
        // first non-blank character.
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

    /// Returns `false` when already on the last visible line (no-op) — used
    /// by the `j` handler to detect a failed move for macro-abort purposes
    /// (#806).
    pub(crate) fn move_down(&mut self) -> bool {
        let max_line = self.buffer().len_lines().saturating_sub(1);
        let mut next = self.view().cursor.line;
        loop {
            if next >= max_line {
                return false;
            }
            next += 1;
            if !self.view().is_line_hidden(next) {
                break;
            }
        }
        let want = self.curswant();
        self.view_mut().cursor.line = next;
        self.apply_curswant(want);
        true
    }

    /// Returns `false` when already on the first visible line (no-op) — see `move_down`.
    pub(crate) fn move_up(&mut self) -> bool {
        let mut prev = self.view().cursor.line;
        loop {
            if prev == 0 {
                return false;
            }
            prev -= 1;
            if !self.view().is_line_hidden(prev) {
                break;
            }
        }
        let want = self.curswant();
        self.view_mut().cursor.line = prev;
        self.apply_curswant(want);
        true
    }

    /// The column a vertical motion should aim for: the remembered
    /// `curswant` if one is set, otherwise the cursor's actual current
    /// column (captured now, before this motion moves the line, so a chain
    /// of `j`/`k` keeps returning to the column the chain *started* at
    /// rather than whatever a shorter intervening line clamped it to).
    pub(crate) fn curswant(&mut self) -> usize {
        let want = self.curswant.unwrap_or(self.view().cursor.col);
        self.curswant = Some(want);
        want
    }

    /// Land the cursor on `want` (a column, or `CURSWANT_EOL` for "end of
    /// line"), clamped to the current line's length. Does NOT touch
    /// `self.curswant` itself — the whole point is that the desired column
    /// survives being clamped so a later vertical motion can return to it.
    pub(crate) fn apply_curswant(&mut self, want: usize) {
        let line = self.view().cursor.line;
        let max_col = self.get_max_cursor_col(line);
        let col = if want == CURSWANT_EOL {
            max_col
        } else {
            want.min(max_col)
        };
        self.view_mut().cursor.col = col;
    }

    /// The buffer line `M` (and `dM`) target: the middle of the lines
    /// actually visible in the window, which for a buffer shorter than the
    /// window is fewer than `viewport_lines()` (#805).
    pub(crate) fn middle_visible_line(&self) -> usize {
        let scroll_top = self.view().scroll_top;
        let viewport = self.viewport_lines().max(1);
        let max_line = self.buffer().len_lines().saturating_sub(1);
        // #805 review: `saturating_sub` here is cheap insurance against a
        // transient `scroll_top > max_line` (e.g. right after the buffer
        // shrinks) underflowing this hot path for both `M` and `dM`.
        let last_visible = (scroll_top + viewport - 1).min(max_line);
        let visible_count = last_visible.saturating_sub(scroll_top) + 1;
        scroll_top + (visible_count - 1) / 2
    }

    /// Vim's `'scroll'` option: sticky line count used by `<C-d>`/`<C-u>`.
    /// Defaults to half the window height when never explicitly set.
    pub(crate) fn effective_scroll(&self) -> usize {
        self.scroll_value
            .unwrap_or_else(|| (self.viewport_lines() / 2).max(1))
    }

    /// `<C-d>`/`<C-u>`: scroll the viewport AND move the cursor by the same
    /// `delta` lines (positive = down), fold-aware and clamped to the
    /// buffer. Column follows `curswant` like any other vertical motion.
    pub(crate) fn scroll_and_move_by(&mut self, delta: isize) {
        let max_line = self.buffer().len_lines().saturating_sub(1);
        let count = delta.unsigned_abs();
        let (new_line, new_top) = if delta >= 0 {
            (
                self.view()
                    .next_visible_line(self.view().cursor.line, count, max_line),
                self.view()
                    .next_visible_line(self.view().scroll_top, count, max_line),
            )
        } else {
            (
                self.view()
                    .prev_visible_line(self.view().cursor.line, count),
                self.view().prev_visible_line(self.view().scroll_top, count),
            )
        };
        self.view_mut().cursor.line = new_line;
        self.view_mut().scroll_top = new_top;
        let want = self.curswant();
        self.apply_curswant(want);
    }

    /// `<C-f>`: scroll a full page forward, keeping a 2-line overlap with
    /// the previous page, and land the cursor on the new top line
    /// (adjusted for `scrolloff`). Fold-aware.
    ///
    /// Once the last buffer line is already visible, real Vim stops
    /// stepping and collapses the window to show just that last line
    /// instead of clamping the usual step (#805: confirmed against real
    /// interactive Neovim in a real terminal, which behaves differently
    /// here than the headless oracle `tests/nvim_conformance.rs` uses —
    /// see `scripts/nvim_headless_vs_interactive_repro.sh`). A step that
    /// merely undershoots `max_line` by a line or two would otherwise miss
    /// this collapse, since it never overshoots far enough to hit
    /// `next_visible_line`'s own clamp.
    pub(crate) fn page_down(&mut self) {
        let viewport = self.viewport_lines().max(1);
        let max_line = self.buffer().len_lines().saturating_sub(1);
        let old_top = self.view().scroll_top;
        if (old_top + viewport).saturating_sub(1) >= max_line {
            self.view_mut().scroll_top = max_line;
            self.view_mut().cursor.line = max_line;
            let want = self.curswant();
            self.apply_curswant(want);
            return;
        }
        let overlap = 2usize.min(viewport.saturating_sub(1));
        let step = viewport.saturating_sub(overlap);
        let new_top = self.view().next_visible_line(old_top, step, max_line);
        self.view_mut().scroll_top = new_top;
        let scrolloff = self.settings.scrolloff;
        self.view_mut().cursor.line = self.view().next_visible_line(new_top, scrolloff, max_line);
        let want = self.curswant();
        self.apply_curswant(want);
    }

    /// `<C-b>`: scroll a full page backward, keeping a 2-line overlap with
    /// the previous page. Mirrors `<C-f>`, with two differences confirmed
    /// against real interactive Neovim (#805; see
    /// `scripts/nvim_headless_vs_interactive_repro.sh` — the headless
    /// oracle `tests/nvim_conformance.rs` uses disagrees with real Neovim
    /// on both):
    ///  - already at the top of the buffer is a true no-op (no cursor
    ///    move at all), not just a clamped scroll;
    ///  - the cursor lands on `scrolloff + 1` lines below the *previous*
    ///    topline — not at the bottom of the new page — which only
    ///    coincides with "bottom of the new page" when the scroll isn't
    ///    clamped by the start of the buffer.
    pub(crate) fn page_up(&mut self) {
        let old_top = self.view().scroll_top;
        if old_top == 0 {
            return;
        }
        let viewport = self.viewport_lines().max(1);
        let max_line = self.buffer().len_lines().saturating_sub(1);
        let overlap = 2usize.min(viewport.saturating_sub(1));
        let step = viewport.saturating_sub(overlap);
        let new_top = self.view().prev_visible_line(old_top, step);
        self.view_mut().scroll_top = new_top;
        let scrolloff = self.settings.scrolloff;
        self.view_mut().cursor.line = (old_top + scrolloff + 1).min(max_line);
        let want = self.curswant();
        self.apply_curswant(want);
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
    /// Candidates for `<C-n>`/`<C-p>` keyword completion, ordered the way
    /// Vim's actual scan is (`:h compl-keyword`): forward distance from the
    /// cursor, wrapping around the buffer. `<C-n>` takes index 0 (nearest
    /// match after the cursor); `<C-p>` takes the last index, which under
    /// this wraparound ordering is the nearest match *before* the cursor —
    /// not alphabetical order, which picks an arbitrary match regardless of
    /// proximity (#804, "C-p completion").
    pub(crate) fn word_completions_for_prefix(&self, prefix: &str) -> Vec<String> {
        let cursor_line = self.view().cursor.line;
        let cursor_col = self.view().cursor.col;
        let cursor_pos = self.buffer().line_to_char(cursor_line) + cursor_col;
        let total = self.buffer().len_chars().max(1);
        let mut best_dist: std::collections::HashMap<String, usize> = Default::default();
        let mut abs = 0usize;
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
                        let occ_pos = abs + start;
                        let dist = if occ_pos >= cursor_pos {
                            occ_pos - cursor_pos
                        } else {
                            occ_pos + total - cursor_pos
                        };
                        best_dist
                            .entry(word)
                            .and_modify(|d| *d = (*d).min(dist))
                            .or_insert(dist);
                    }
                } else {
                    i += 1;
                }
            }
            abs += len;
        }
        let mut v: Vec<(String, usize)> = best_dist.into_iter().collect();
        v.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        v.into_iter().map(|(w, _)| w).collect()
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

    /// Handle a mouse click on the completion popup.
    /// Returns `true` if the click was consumed (inside the popup).
    pub fn handle_completion_click(&mut self, hit: quadraui::CompletionsHit) -> bool {
        match hit {
            quadraui::CompletionsHit::Item(idx) => {
                self.apply_completion_candidate(idx);
                self.dismiss_completion();
                true
            }
            quadraui::CompletionsHit::Inert => {
                self.dismiss_completion();
                true
            }
            quadraui::CompletionsHit::Empty => {
                self.dismiss_completion();
                false
            }
        }
    }

    /// Dismiss the completion popup and cancel any pending LSP completion request.
    /// This ensures that a late-arriving LSP response cannot re-show a popup
    /// after the user has already dismissed it (e.g. by pressing Escape or
    /// moving the cursor).
    pub(crate) fn dismiss_completion(&mut self) {
        self.completion_candidates.clear();
        self.completion_idx = None;
        self.completion_display_only = false;
        self.completion_filter_prefix.clear();
        self.lsp_pending_completion = None;
    }

    /// Trigger completion popup based on current cursor prefix.
    /// Called after each text change in Insert mode (`manual = false`) or
    /// from an explicit user gesture like Ctrl+Space (`manual = true`).
    ///
    /// Empty-prefix behavior differs: auto triggers dismiss the popup (typing
    /// shouldn't drown the user in every word in scope), while manual triggers
    /// fall through to the LSP so the user sees all in-scope symbols (VSCode
    /// parity).
    ///
    /// When the new prefix **extends** the previous one (`S` → `Sc`), existing
    /// candidates are narrowed by `starts_with(new_prefix)` and merged with
    /// new buffer-word hits, instead of being wholesale-replaced. This keeps
    /// items the user already saw from disappearing when a fresh nearby-word
    /// scan or LSP request happens to return a smaller set (#467).
    pub(crate) fn trigger_completion(&mut self, manual: bool) {
        let (prefix, _) = self.completion_prefix_at_cursor();
        if prefix.is_empty() && !manual {
            self.dismiss_completion();
            return;
        }
        let prev_prefix = std::mem::take(&mut self.completion_filter_prefix);
        let extends = !prev_prefix.is_empty() && prefix.starts_with(&prev_prefix);
        if prefix.is_empty() {
            // Manual trigger with no prefix: skip the buffer-word scan (it would
            // match every word in the nearby radius) and rely on the LSP response.
            self.completion_start_col = self.view().cursor.col;
            self.completion_candidates.clear();
            self.completion_idx = None;
            self.completion_display_only = false;
        } else {
            // Narrow existing candidates by the new prefix when it extends the
            // previous one; otherwise start fresh.
            if extends {
                self.completion_candidates
                    .retain(|c| c.starts_with(&prefix));
            } else {
                self.completion_candidates.clear();
            }
            // Use a fast nearby-lines scan instead of scanning the entire buffer.
            // For a 15K-line file, full scan takes 270ms; nearby scan is ~1ms.
            for word in self.word_completions_nearby(&prefix) {
                if !self.completion_candidates.iter().any(|c| c == &word) {
                    self.completion_candidates.push(word);
                }
            }
            if !self.completion_candidates.is_empty() {
                self.completion_start_col = self.view().cursor.col - prefix.chars().count();
                self.completion_idx = Some(0);
                self.completion_display_only = true;
            } else {
                self.completion_idx = None;
                self.completion_display_only = false;
            }
        }
        self.completion_filter_prefix = prefix;
        // Async LSP source — response will update candidates if popup is still active
        self.lsp_request_completion();
    }

    /// True when an insert-mode keypress would be consumed by completion
    /// handling — used by backends to suppress global accelerators that
    /// would otherwise win the dispatch race (#287). Mirrors the gates in
    /// `handle_insert_key`:
    ///
    /// - `Ctrl+N` / `Ctrl+P` — always (start or cycle word completion).
    /// - `Down` / `Up` — only while a display-only popup is active.
    /// - The configured accept key (`completion_keys.accept`, mode-derived —
    ///   `<Tab>` in Vscode mode, `<C-y>` in Vim mode — see #800) — only while
    ///   a display-only popup is active.
    ///
    /// Dead in ShellApp mode until completion key intercept is re-wired (#448-C follow-on).
    #[allow(dead_code)]
    pub fn insert_completion_intercepts_key(&self, key_name: &str, ctrl: bool) -> bool {
        if self.mode != Mode::Insert {
            return false;
        }
        if ctrl && (key_name == "n" || key_name == "p") {
            return true;
        }
        let popup_active = self.completion_display_only && self.completion_idx.is_some();
        if !popup_active {
            return false;
        }
        if !ctrl && (key_name == "Down" || key_name == "Up") {
            return true;
        }
        let accept_key = self
            .settings
            .completion_keys
            .accept(self.settings.editor_mode);
        Self::key_matches_binding(&accept_key, ctrl, key_name)
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
    ///
    /// Decreases `scroll_left`, which exposes more of the line to the left of
    /// the viewport (cursor's visible position shifts right). The cursor is
    /// pulled onto the new viewport if the scroll has left it off-screen — if
    /// we didn't do that, `ensure_cursor_visible` (called after every key in
    /// `handle_key`) would snap `scroll_left` back to track the cursor and
    /// effectively undo the scroll.
    pub(crate) fn scroll_left_by(&mut self, count: usize) {
        let sl = self.view().scroll_left;
        let new_sl = sl.saturating_sub(count);
        self.view_mut().scroll_left = new_sl;
        self.clamp_cursor_to_horizontal_viewport(new_sl);
    }

    /// zl — scroll view right by `count` columns.
    ///
    /// Increases `scroll_left`, which exposes more of the line to the right of
    /// the viewport. The cursor is pulled onto the new viewport so that
    /// `ensure_cursor_visible` does not snap the scroll back (see
    /// `scroll_left_by`).
    pub(crate) fn scroll_right_by(&mut self, count: usize) {
        let new_sl = self.view().scroll_left + count;
        self.view_mut().scroll_left = new_sl;
        self.clamp_cursor_to_horizontal_viewport(new_sl);
    }

    /// Move the cursor's column onto the horizontal viewport at `scroll_left`.
    ///
    /// If `viewport_cols` is 0 (uninitialised), this is a no-op so that
    /// headless / uninitialised engines still let scroll commands shift
    /// `scroll_left` freely. Otherwise, the cursor column is clamped to
    /// `[scroll_left, scroll_left + viewport_cols)`. This is the cursor
    /// adjustment Vim performs for `zh`/`zl`/`zH`/`zL` when the scroll would
    /// leave the cursor off-screen.
    fn clamp_cursor_to_horizontal_viewport(&mut self, scroll_left: usize) {
        let viewport_cols = self.view().viewport_cols;
        if viewport_cols == 0 {
            return;
        }
        let right_edge = scroll_left + viewport_cols;
        let cursor_col = self.view().cursor.col;
        if cursor_col < scroll_left {
            self.view_mut().cursor.col = scroll_left;
        } else if cursor_col >= right_edge {
            self.view_mut().cursor.col = right_edge - 1;
        }
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
        // `"_` is the black hole register (`:h quote_`): writes to it vanish
        // entirely — crucially, they do NOT fall through to the unnamed
        // register the way every other named register does below. This is
        // what makes `"_dd` (or `viw"_dP`, `:d _`) leave `""`/`"1`-`"9`
        // untouched instead of clobbering them (#806).
        if reg == '_' {
            return;
        }
        // Uppercase register (A-Z): append to lowercase register
        if reg.is_ascii_uppercase() {
            let lower = reg.to_ascii_lowercase();
            let (existing, existing_lw) = self.registers.get(&lower).cloned().unwrap_or_default();
            let combined_lw = existing_lw || is_linewise;
            let combined = if existing.is_empty() {
                content.clone()
            } else if existing_lw {
                // Existing is linewise: already ends with '\n', just concatenate.
                format!("{}{}", existing, content)
            } else if is_linewise {
                // Charwise existing + linewise append: the result becomes
                // linewise, so it needs the line break the append itself
                // doesn't carry yet.
                format!("{}\n{}", existing, content)
            } else {
                // Charwise + charwise: appending is plain concatenation, no
                // separator (`:h quote_alpha`) — a `\n` here was corrupting
                // `"Ayw` appends with a line break neither piece had (#806,
                // `reg:"ayw "Ayw "ap`).
                format!("{}{}", existing, content)
            };
            self.registers
                .insert(lower, (combined.clone(), combined_lw));
            self.registers.insert('"', (combined, combined_lw));
            return;
        }
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

    /// Sets a yank register. Like set_register, but ALSO updates "0 when the
    /// target is the unnamed register. Yanks to a named register (e.g. "ayy)
    /// leave "0 untouched, matching Vim's :help registers semantics.
    pub(crate) fn set_yank_register(&mut self, reg: char, content: String, is_linewise: bool) {
        self.set_register(reg, content.clone(), is_linewise);
        if reg == '"' {
            // "0 is the yank-only register — set on every unnamed yank, never on
            // deletes or on yanks to an explicit named register.
            self.registers.insert('0', (content, is_linewise));
        }
    }

    /// Sets a delete register. Like set_register, but:
    /// - Linewise / multi-line: shifts "1"-"8" → "2"-"9", sets "1".
    /// - Character (< 1 line): sets "-" (small-delete register).
    pub(crate) fn set_delete_register(&mut self, reg: char, content: String, is_linewise: bool) {
        self.set_delete_register_impl(reg, content, is_linewise, false);
    }

    /// Like [`set_delete_register`], but always goes through the numbered
    /// `"1"`-`"9"` chain even when the deleted text is less than one line.
    /// Real Vim carves out this exception specifically for deletes made with
    /// `%`, `` ` ``, `/`, `?`, `n`, `N`, `(`, `)`, `{`, `}` — motions that
    /// aren't "smaller than a line" deletes even when their result happens to
    /// be (`:h quotedash`: "This does not happen for the delete operator
    /// with a few specific motions, or a specific register was used with the
    /// delete."). Ordinary char motions (`dw`, `x`, `dl`, …) still use the
    /// small-delete `"-` register via `set_delete_register` (#806).
    pub(crate) fn set_delete_register_special_motion(
        &mut self,
        reg: char,
        content: String,
        is_linewise: bool,
    ) {
        self.set_delete_register_impl(reg, content, is_linewise, true);
    }

    fn set_delete_register_impl(
        &mut self,
        reg: char,
        content: String,
        is_linewise: bool,
        force_numbered: bool,
    ) {
        self.set_register(reg, content.clone(), is_linewise);
        // Black hole: no register bookkeeping of any kind (see `set_register`).
        if reg == '_' {
            return;
        }
        if is_linewise || content.contains('\n') || force_numbered {
            // Multi-line delete: shift numbered registers down
            for i in (1usize..=8).rev() {
                let from = char::from_digit(i as u32, 10).unwrap();
                let to = char::from_digit((i + 1) as u32, 10).unwrap();
                if let Some(val) = self.registers.get(&from).cloned() {
                    self.registers.insert(to, val);
                }
            }
            self.registers.insert('1', (content, is_linewise));
        } else if !content.is_empty() && reg == '"' {
            // Small character delete: set "-" register — but only for an
            // unnamed delete. An explicit register (`"adw`) does NOT also
            // touch "- (#806, `reg:"adw does not set "-`), even though a
            // multi-line/linewise explicit-register delete DOES still touch
            // "1-"9 above (verified against real Vim: `"add` sets both "a
            // and "1; `"bdw` sets only "b, leaving "- untouched).
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

    /// Evaluate the text typed into the `"=`/`<C-r>=` expression register and
    /// return its result formatted as it would be inserted/pasted, or an
    /// error message.
    ///
    /// Real Vim runs the full Vimscript expression evaluator here. VimCode
    /// doesn't embed one, so this covers only integer arithmetic (`+ - * /
    /// %`, unary minus, parens) — enough for the "do a little math and paste
    /// the answer" idiom the register exists for (#806). Anything fancier
    /// (string concatenation, function calls, variables) reports an error
    /// rather than silently doing the wrong thing.
    pub(crate) fn eval_expr_register(src: &str) -> Result<String, String> {
        eval_expr_register_arith(src).map(|n| n.to_string())
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
        self.macro_recording_append = false;
        self.message = format!("Recording macro into register '{}'", register);
    }

    /// Start recording a macro that appends to `register`'s existing content
    /// instead of overwriting it (`qA` — `register` is already the lowercase
    /// target, #806 "mac:qA append").
    pub(crate) fn start_macro_recording_append(&mut self, register: char) {
        self.macro_recording = Some(register);
        self.recording_buffer.clear();
        self.macro_recording_append = true;
        self.message = format!("Recording macro into register '{}' (appending)", register);
    }

    /// Stop recording and save the macro to the register.
    pub(crate) fn stop_macro_recording(&mut self) {
        if let Some(reg) = self.macro_recording {
            // Convert recording_buffer to string
            let macro_content: String = self.recording_buffer.iter().collect();

            // Store in register (not linewise). `set_register`'s uppercase
            // form does a plain charwise concat onto the existing lowercase
            // content — exactly `qA`'s append semantics — so route through
            // that instead of duplicating the combine logic here.
            if self.macro_recording_append {
                self.set_register(reg.to_ascii_uppercase(), macro_content, false);
            } else {
                self.set_register(reg, macro_content, false);
            }

            self.message = format!("Macro recorded into register '{}'", reg);
            self.macro_recording = None;
            self.recording_buffer.clear();
            self.macro_recording_append = false;
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

            // Cursor on last replaced char (Neovim behavior)
            self.view_mut().cursor.col = col + to_replace - 1;
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
    ///
    /// `count` repeats the *whole register content* `count` times as a single
    /// paste (`:h p`) — NOT `count` separate paste operations. The two differ
    /// for a multi-line register: `2yy3p` must produce three back-to-back
    /// copies of the two yanked lines, not each yanked line tripled in place
    /// (the latter is what calling this function in a loop produces, since
    /// each call re-pastes after wherever the *previous* call's cursor
    /// landed — #806, "misc:2yy 3p").
    pub fn paste_after(&mut self, count: usize, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register_content(reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                return;
            }
        };
        let content = if count > 1 {
            content.repeat(count)
        } else {
            content
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
    /// `count` repeats the register content as a single block — see `paste_after`.
    pub(crate) fn paste_before(&mut self, count: usize, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register_content(reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                return;
            }
        };
        let content = if count > 1 {
            content.repeat(count)
        } else {
            content
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
    /// `count` repeats the register content as a single block — see `paste_after`.
    pub(crate) fn paste_after_cursor_after(&mut self, count: usize, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register_content(reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                return;
            }
        };
        let content = if count > 1 {
            content.repeat(count)
        } else {
            content
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
    /// `count` repeats the register content as a single block — see `paste_after`.
    pub(crate) fn paste_before_cursor_after(&mut self, count: usize, changed: &mut bool) {
        let reg = self.active_register();
        let (content, is_linewise) = match self.get_register_content(reg) {
            Some(pair) => pair,
            None => {
                self.clear_selected_register();
                return;
            }
        };
        let content = if count > 1 {
            content.repeat(count)
        } else {
            content
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

        // Vim: J joins 2 lines (1 join), 3J joins 3 lines (2 joins).
        let joins = if count <= 1 { 1 } else { count - 1 };
        let joins = joins.min(total_lines.saturating_sub(start_line + 1));
        if joins == 0 {
            return;
        }

        self.start_undo_group();
        let mut join_col = 0usize; // track the join point for cursor placement
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

            // Insert a space unless the next non-ws char is ')' or next line was empty/only ws.
            // Also don't add space if the current line already ends with whitespace.
            let should_add_space = !matches!(next_non_ws, None | Some(')') | Some(']') | Some('}'));
            let ends_with_ws = newline_pos > cur_line_start
                && self.buffer().content.char(newline_pos - 1).is_whitespace();
            let insert_space = should_add_space && !ends_with_ws;

            // A join needs the absorbed line's marks to gain a *column*
            // offset (not just shift down a line, which is all the generic
            // `delete_with_undo`/`insert_with_undo` hook does) — precise
            // offset-based fixup, done by hand around the raw splice
            // (#806, "mark:`a after line join").
            let (local_marks, global_marks) = self.snapshot_marks_as_offsets();
            self.suppress_mark_line_adjust = true;
            self.delete_with_undo(newline_pos, del_end);
            if insert_space {
                self.insert_with_undo(newline_pos, " ");
            }
            self.suppress_mark_line_adjust = false;
            let ins_len = if insert_space { 1 } else { 0 };
            self.restore_marks_from_offsets(
                local_marks,
                global_marks,
                newline_pos,
                del_end - newline_pos,
                ins_len,
            );

            if insert_space {
                // Cursor at the inserted space
                join_col = newline_pos - cur_line_start;
            } else {
                // No space inserted — cursor at last char before where next line starts
                join_col = (newline_pos - cur_line_start).saturating_sub(1);
            }
        }
        self.finish_undo_group();

        // Cursor at the join point (where the space was inserted)
        self.view_mut().cursor.col = join_col;
        self.clamp_cursor_col();
        *changed = true;
    }

    // =======================================================================
    // Scroll cursor to position (zz / zt / zb)
    // =======================================================================

    /// Scroll so that cursor line is centered in viewport.
    pub(crate) fn scroll_cursor_center(&mut self) {
        let cursor_line = self.view().cursor.line;
        // #805: `(viewport - 1) / 2`, not `viewport / 2` — confirmed against
        // real interactive Neovim's `zz`, which was landing one line lower
        // than vimcode did (see scripts/nvim_headless_vs_interactive_repro.sh).
        let half = self.viewport_lines().saturating_sub(1) / 2;
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

    /// Build a `JumpEntry` snapshot of the engine's current cursor position
    /// and the pane (group/tab/window) it's in.
    pub(crate) fn current_jump_entry(&self) -> JumpEntry {
        JumpEntry {
            file: self.active_buffer_state().file_path.clone(),
            line: self.view().cursor.line,
            col: self.view().cursor.col,
            group_id: self.active_group,
            tab_id: self.active_tab().id,
            window_id: self.active_window_id(),
        }
    }

    /// Push the current cursor position onto the jump list, and set it as
    /// the `''`/`` `` `` mark (pcmark).
    pub fn push_jump_location(&mut self) {
        // Save pre-jump position for '' / `` marks
        let line = self.view().cursor.line;
        let col = self.view().cursor.col;
        self.last_jump_pos = Some((line, col));
        self.append_jump_list_entry();
    }

    /// The jumplist-only half of `push_jump_location` — appends the current
    /// cursor position, deduped against the top entry and truncating any
    /// forward (redo) history. Split out so `record_jump_from` can update
    /// the `''` mark unconditionally while only conditionally touching the
    /// persistent list (#806).
    fn append_jump_list_entry(&mut self) {
        let entry = self.current_jump_entry();

        // Truncate forward history when a new jump is made
        if self.jump_list_pos < self.jump_list.len() {
            self.jump_list.truncate(self.jump_list_pos);
        }

        // Don't push a duplicate of the current top entry
        if self.jump_list.last() == Some(&entry) {
            return;
        }

        self.jump_list.push(entry);

        // Cap at 100 entries
        if self.jump_list.len() > 100 {
            self.jump_list.remove(0);
        }

        self.jump_list_pos = self.jump_list.len();
    }

    /// Lazily seed the jumplist with the startup position the first time
    /// `<C-o>` is asked to go somewhere and the list is otherwise empty.
    /// Real Neovim behaves this way for line-changing-but-not-jump-worthy
    /// motions (`j`/`k`, even `20j`): `getjumplist()` reports empty right
    /// after such a motion, yet a bare `<C-o>` still returns to the
    /// position editing started at — but ONLY once the cursor has actually
    /// left that starting *line*; a same-line motion (`%` on a one-line
    /// match, `(` within a sentence) leaves `<C-o>` a genuine no-op (#806:
    /// "jump:C-o after j 20 lines" needs the seed, "jump:% C-o" / "jump:C-o
    /// after (" must NOT get one). Checking "has the line changed" here,
    /// right before the fallback would otherwise report "already at
    /// oldest", reproduces both without a real jump command ever having to
    /// decide it retroactively.
    fn seed_jump_list_if_line_left(&mut self) {
        if !self.jump_list.is_empty() {
            return;
        }
        if let Some(entry) = &self.startup_jump_entry {
            if entry.line != self.view().cursor.line {
                self.jump_list.push(entry.clone());
                self.jump_list_pos = self.jump_list.len();
            }
        }
    }

    /// Record a jump that started at `pre_cursor` (cursor has already moved
    /// to its destination by the time this is called). Always updates the
    /// `''`/`` `` `` pcmark, but only appends a *persistent jumplist* entry
    /// when the move actually changed line. Verified against real Neovim: a
    /// same-line `%`/`(`/`)` still moves the pcmark (`` `` `` returns to the
    /// exact starting column) even though `getjumplist()` stays untouched
    /// (#806, "mark:`` after %" vs "jump:% C-o").
    pub(crate) fn record_jump_from(&mut self, pre_cursor: Cursor) {
        self.last_jump_pos = Some((pre_cursor.line, pre_cursor.col));
        if self.view().cursor.line == pre_cursor.line {
            return;
        }
        let post_cursor = self.view().cursor;
        self.view_mut().cursor = pre_cursor;
        self.append_jump_list_entry();
        self.view_mut().cursor = post_cursor;
    }

    /// Navigate backward in the jump list (Ctrl-O).
    pub fn jump_list_back(&mut self) {
        self.seed_jump_list_if_line_left();
        // When at the "live" end (not stored in list), save current position
        // so Ctrl-I can return to it, then jump to the previous entry.
        if self.jump_list_pos == self.jump_list.len() {
            if self.jump_list.is_empty() {
                self.message = "Already at oldest position in jump list".to_string();
                return;
            }
            let entry = self.current_jump_entry();
            let should_push = self.jump_list.last() != Some(&entry);
            if should_push {
                self.jump_list.push(entry);
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
    ///
    /// Restores **by lookup, not by open** (#674): if the entry's
    /// group/tab/window still exist, switch to that exact pane — the same
    /// tab/split gets activated, not a fresh copy of the file opened into
    /// whatever pane is currently focused. Only when the pane is gone
    /// (its tab or split was closed) does this fall back to
    /// `open_file_with_mode`, which opens the file into the *current*
    /// window; that fallback is the recovery path, not the normal one.
    ///
    /// A recorded window can also still exist, still belong to the same
    /// tab, and yet no longer show the recorded file — `:e`, `gf`, and the
    /// explorer/fuzzy-finder all replace a window's buffer **in place**,
    /// keeping the `WindowId` the same. `jump_pane_buffer_matches` catches
    /// that case (most commonly: single window, no splits/tabs, file A then
    /// file B opened into the same pane) and re-opens the recorded file
    /// into that exact window rather than trusting a pane match that no
    /// longer holds the right buffer.
    ///
    /// The jump list is deliberately global rather than per-window (stock
    /// Vim keeps one jumplist per window — see `:help jumplist`). This repo
    /// chose global because the reported use case is explicitly
    /// cross-tab/cross-pane: walking Ctrl-O back through time should surface
    /// the tab you were in a few jumps ago, not stop dead at the pane
    /// boundary. See VIM_COMPATIBILITY.md for the recorded decision.
    pub(crate) fn apply_jump_list_entry(&mut self, idx: usize) {
        let entry = match self.jump_list.get(idx) {
            Some(e) => e.clone(),
            None => return,
        };

        let pane = self
            .locate_jump_pane(entry.group_id, entry.tab_id, entry.window_id)
            .filter(|_| self.jump_pane_buffer_matches(entry.window_id, &entry.file));

        if let Some((group_id, tab_idx)) = pane {
            self.switch_to_jump_pane(group_id, tab_idx, entry.window_id);
        } else {
            // Pane is gone, or it exists but its buffer was swapped in
            // place since the jump was recorded — recover by opening the
            // file into the current window.
            let current_file = self.active_buffer_state().file_path.clone();
            if entry.file != current_file {
                if let Some(path) = &entry.file {
                    let _ = self.open_file_with_mode(path, OpenMode::Permanent);
                }
            }
        }

        let max_line = self.buffer().len_lines().saturating_sub(1);
        self.view_mut().cursor.line = entry.line.min(max_line);
        self.view_mut().cursor.col = entry.col;
        self.clamp_cursor_col();
    }

    // =======================================================================
    // Indent / Dedent (>> / <<)
    // =======================================================================

    /// Indent `count` lines starting at `start_line` by shift_width.
    pub(crate) fn indent_lines(&mut self, start_line: usize, count: usize, changed: &mut bool) {
        let sw = self.effective_shift_width();
        let ts = (self.settings.tabstop as usize).max(1);
        let expand = self.settings.expand_tab;

        self.start_undo_group();
        let total = self.buffer().len_lines();
        for i in 0..count {
            let line_idx = start_line + i;
            if line_idx >= total {
                break;
            }
            let line_content: String = self.buffer().content.line(line_idx).chars().collect();
            let body = line_content.trim_end_matches(['\n', '\r']);

            // Measure the existing indent in *display columns* — a tab advances
            // to the next 'tabstop', which is not the same as 'shiftwidth'.
            let mut cols = 0usize;
            let mut ws_chars = 0usize;
            for ch in body.chars() {
                match ch {
                    ' ' => {
                        cols += 1;
                        ws_chars += 1;
                    }
                    '\t' => {
                        cols += ts - (cols % ts);
                        ws_chars += 1;
                    }
                    _ => break,
                }
            }

            let new_cols = cols + sw;
            // `noet` only uses a tab where a *whole* tabstop fits, so with
            // ts=8 / sw=4 Vim indents with four spaces, not a tab.
            let new_indent = if expand {
                " ".repeat(new_cols)
            } else {
                format!(
                    "{}{}",
                    "\t".repeat(new_cols / ts),
                    " ".repeat(new_cols % ts)
                )
            };

            let line_start = self.buffer().line_to_char(line_idx);
            if ws_chars > 0 {
                self.delete_with_undo(line_start, line_start + ws_chars);
            }
            self.insert_with_undo(line_start, &new_indent);
        }
        self.finish_undo_group();
        // `'[`/`` `[ `` and `']`/`` `] `` after a shift command (#806, "mark:'[
        // after >>"). Narrower than real Vim's `[`/`]` (which track every
        // change/yank/paste — see `last_change_start`'s doc comment).
        self.last_change_start = Some((start_line, 0));
        let end_line = (start_line + count.saturating_sub(1)).min(total.saturating_sub(1));
        let end_col = self.buffer().line_len_chars(end_line).saturating_sub(1);
        self.last_change_end = Some((end_line, end_col));
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
            // See `indent_lines`'s matching comment (#806, "mark:'[ after >>").
            self.last_change_start = Some((start_line, 0));
            let end_line = (start_line + count.saturating_sub(1)).min(total.saturating_sub(1));
            let end_col = self.buffer().line_len_chars(end_line).saturating_sub(1);
            self.last_change_end = Some((end_line, end_col));
        }
    }
}

/// Minimal recursive-descent integer arithmetic parser backing
/// [`Engine::eval_expr_register`] — `:h expr-register`, scoped down to
/// `+ - * / %`, unary minus, and parens (#806). `/` and `%` truncate toward
/// zero, matching Vimscript integer division.
fn eval_expr_register_arith(src: &str) -> Result<i64, String> {
    struct Parser<'a> {
        chars: std::iter::Peekable<std::str::Chars<'a>>,
    }

    impl Parser<'_> {
        fn skip_ws(&mut self) {
            while matches!(self.chars.peek(), Some(c) if c.is_whitespace()) {
                self.chars.next();
            }
        }

        fn expr(&mut self) -> Result<i64, String> {
            let mut val = self.term()?;
            loop {
                self.skip_ws();
                match self.chars.peek() {
                    Some('+') => {
                        self.chars.next();
                        val += self.term()?;
                    }
                    Some('-') => {
                        self.chars.next();
                        val -= self.term()?;
                    }
                    _ => break,
                }
            }
            Ok(val)
        }

        fn term(&mut self) -> Result<i64, String> {
            let mut val = self.unary()?;
            loop {
                self.skip_ws();
                match self.chars.peek() {
                    Some('*') => {
                        self.chars.next();
                        val *= self.unary()?;
                    }
                    Some('/') => {
                        self.chars.next();
                        let rhs = self.unary()?;
                        if rhs == 0 {
                            return Err("divide by zero".to_string());
                        }
                        val /= rhs;
                    }
                    Some('%') => {
                        self.chars.next();
                        let rhs = self.unary()?;
                        if rhs == 0 {
                            return Err("divide by zero".to_string());
                        }
                        val %= rhs;
                    }
                    _ => break,
                }
            }
            Ok(val)
        }

        fn unary(&mut self) -> Result<i64, String> {
            self.skip_ws();
            match self.chars.peek() {
                Some('-') => {
                    self.chars.next();
                    Ok(-self.unary()?)
                }
                Some('+') => {
                    self.chars.next();
                    self.unary()
                }
                _ => self.atom(),
            }
        }

        fn atom(&mut self) -> Result<i64, String> {
            self.skip_ws();
            if let Some('(') = self.chars.peek() {
                self.chars.next();
                let val = self.expr()?;
                self.skip_ws();
                if self.chars.peek() == Some(&')') {
                    self.chars.next();
                } else {
                    return Err("expected ')'".to_string());
                }
                return Ok(val);
            }
            let mut digits = String::new();
            while matches!(self.chars.peek(), Some(c) if c.is_ascii_digit()) {
                digits.push(self.chars.next().unwrap());
            }
            if digits.is_empty() {
                return Err("expected a number".to_string());
            }
            digits.parse::<i64>().map_err(|e| e.to_string())
        }
    }

    let mut parser = Parser {
        chars: src.chars().peekable(),
    };
    let val = parser.expr()?;
    parser.skip_ws();
    if parser.chars.peek().is_some() {
        return Err("trailing characters in expression".to_string());
    }
    Ok(val)
}

// ---------------------------------------------------------------------------
// `<C-a>` / `<C-x>` — a port of Vim's `do_addsub()` + `vim_str2nr()` (#807)
//
// The pre-#807 implementation parsed into `i64` and reformatted with Rust's
// `{}`/`{:x}`, which got five separate things wrong: it dropped leading zeros
// (`009` → `1`), always treated a leading-zero run as octal even though
// Neovim's default 'nrformats' is `bin,hex` (`0099<C-x>` underflowed to
// `1777777777777777777777`), had no binary support, lost the case of hex
// digits and of the `0X` prefix, and could not represent Vim's u64 wraparound
// (`0x0<C-x>` → `0xffffffffffffffff`).  Vim's arithmetic is **unsigned 64-bit
// plus a separate sign flag**, so that is what this models.
// ---------------------------------------------------------------------------

/// Vim's 'nrformats'.  VimCode has no setting for it yet and pins Neovim's
/// default (`bin,hex` — note Vim's own default additionally includes `octal`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct NrFormats {
    pub bin: bool,
    pub oct: bool,
    pub hex: bool,
    pub alpha: bool,
}

impl Default for NrFormats {
    fn default() -> Self {
        Self {
            bin: true,
            oct: false,
            hex: true,
            alpha: false,
        }
    }
}

fn is_bin_digit(c: char) -> bool {
    c == '0' || c == '1'
}

/// Char at `i`, or `'\0'` past the end — mirrors C string indexing so the
/// ported bounds conditions stay readable.
fn at(chars: &[char], i: usize) -> char {
    chars.get(i).copied().unwrap_or('\0')
}

/// Parsed shape of a numeric literal, as produced by Vim's `vim_str2nr()`.
struct ParsedNum {
    /// `Some('x' | 'X' | 'b' | 'B')` for a prefixed literal, `Some('0')` for
    /// octal, `None` for plain decimal.
    pre: Option<char>,
    /// Characters consumed, *including* any leading sign and prefix.
    len: usize,
    /// Magnitude, ignoring the sign.
    value: u64,
    /// The literal did not fit in a `u64` and was saturated to `u64::MAX`.
    overflow: bool,
}

/// Port of `vim_str2nr()`, limited to what `do_addsub` asks of it.
/// Parses at most `maxlen` characters starting at `start`.
fn str2nr(chars: &[char], start: usize, nf: NrFormats, maxlen: usize) -> ParsedNum {
    let end = (start + maxlen).min(chars.len());
    let get = |i: usize| -> char {
        if i < end {
            at(chars, i)
        } else {
            '\0'
        }
    };
    let mut i = start;
    if get(i) == '-' || get(i) == '+' {
        i += 1;
    }
    let mut pre = None;
    if get(i) == '0' {
        let c1 = get(i + 1);
        if nf.hex && (c1 == 'x' || c1 == 'X') && get(i + 2).is_ascii_hexdigit() {
            pre = Some(c1);
            i += 2;
        } else if nf.bin && (c1 == 'b' || c1 == 'B') && is_bin_digit(get(i + 2)) {
            pre = Some(c1);
            i += 2;
        } else if nf.oct && c1 != '8' && c1 != '9' {
            // Octal only when every following digit is in 0..=7; a trailing
            // `8`/`9` makes the whole run decimal again.
            let mut j = i + 1;
            while ('0'..='7').contains(&get(j)) {
                j += 1;
            }
            if get(j) != '8' && get(j) != '9' {
                pre = Some('0');
            }
        }
    }
    let radix: u32 = match pre {
        Some('x') | Some('X') => 16,
        Some('b') | Some('B') => 2,
        Some('0') => 8,
        _ => 10,
    };
    let mut value: u64 = 0;
    let mut overflow = false;
    let mut any = false;
    while let Some(d) = get(i).to_digit(radix) {
        any = true;
        value = match value
            .checked_mul(radix as u64)
            .and_then(|v| v.checked_add(d as u64))
        {
            Some(v) => v,
            None => {
                overflow = true;
                u64::MAX
            }
        };
        i += 1;
    }
    if !any {
        // e.g. a bare `0x` with no hex digits — `pre` was never set in that
        // case, so this only happens on an empty run.
        return ParsedNum {
            pre: None,
            len: 0,
            value: 0,
            overflow: false,
        };
    }
    ParsedNum {
        pre,
        len: i - start,
        value,
        overflow,
    }
}

/// Port of Vim's `do_addsub()` for a single line.
///
/// `chars` is the line without its trailing newline, `cursor_col` the char
/// index the operation starts from, `delta` the signed amount (`<C-x>` passes
/// a negative value), and `sel` the length of the Visual-mode selection on
/// this line (`None` in Normal mode).
///
/// Returns `(replace_start, replace_len, new_text)`, or `None` when there is
/// no number to change.
pub(crate) fn addsub_in_line(
    chars: &[char],
    cursor_col: usize,
    delta: i64,
    nf: NrFormats,
    sel: Option<usize>,
) -> Option<(usize, usize, String)> {
    if chars.is_empty() || delta == 0 {
        return None;
    }
    let visual = sel.is_some();
    let mut col = cursor_col.min(chars.len());

    if !visual {
        // Vim scans backwards over binary then hexadecimal digits so the
        // cursor can sit anywhere *inside* a `0x..`/`0b..` literal, then
        // rescans decimally when that overshot a plain decimal number.
        if nf.bin {
            while col > 0 && is_bin_digit(at(chars, col)) {
                col -= 1;
            }
        }
        if nf.hex {
            while col > 0 && at(chars, col).is_ascii_hexdigit() {
                col -= 1;
            }
        }
        let on_hex_prefix = |c: usize| {
            c > 0
                && (at(chars, c) == 'x' || at(chars, c) == 'X')
                && at(chars, c - 1) == '0'
                && at(chars, c + 1).is_ascii_hexdigit()
        };
        let on_bin_prefix = |c: usize| {
            c > 0
                && (at(chars, c) == 'b' || at(chars, c) == 'B')
                && at(chars, c - 1) == '0'
                && is_bin_digit(at(chars, c + 1))
        };
        if nf.bin && nf.hex && !on_hex_prefix(col) {
            // Binary/hex pattern overlap (`0b101` is also valid hex) — rescan.
            col = cursor_col.min(chars.len());
            while col > 0 && at(chars, col).is_ascii_digit() {
                col -= 1;
            }
        }
        if (nf.hex && on_hex_prefix(col)) || (nf.bin && on_bin_prefix(col)) {
            col -= 1;
        } else {
            // Search forward for a digit, then back to the start of its run.
            col = cursor_col.min(chars.len());
            while col < chars.len()
                && !at(chars, col).is_ascii_digit()
                && !(nf.alpha && at(chars, col).is_ascii_alphabetic())
            {
                col += 1;
            }
            while col > 0
                && at(chars, col - 1).is_ascii_digit()
                && !(nf.alpha && at(chars, col).is_ascii_alphabetic())
            {
                col -= 1;
            }
        }
    }

    let mut negative = false;
    let mut remaining = sel.unwrap_or(usize::MAX);
    if let Some(sel_len) = sel {
        // Confine the forward search to the selection.
        let pos_col = cursor_col;
        remaining = sel_len;
        while col < chars.len()
            && remaining > 0
            && !at(chars, col).is_ascii_digit()
            && !(nf.alpha && at(chars, col).is_ascii_alphabetic())
        {
            col += 1;
            remaining -= 1;
        }
        if remaining == 0 || col >= chars.len() {
            return None;
        }
        // Only a `-` that is itself selected acts as a sign.
        if col > pos_col && at(chars, col - 1) == '-' {
            negative = true;
        }
    }

    let firstdigit = at(chars, col);
    if !firstdigit.is_ascii_digit() && !(nf.alpha && firstdigit.is_ascii_alphabetic()) {
        return None;
    }

    if !visual && col > 0 && at(chars, col - 1) == '-' {
        col -= 1;
        negative = true;
        remaining = usize::MAX;
    }

    let maxlen = if visual {
        remaining.min(chars.len() - col)
    } else {
        chars.len() - col
    };
    let parsed = str2nr(chars, col, nf, maxlen);
    if parsed.len == 0 {
        return None;
    }
    let mut len = parsed.len;
    // A leading `-` is not part of a hex/octal/binary literal: leave it in the
    // buffer and rewrite only the digits (`-0x1<C-a>` → `-0x2`).
    if !visual && parsed.pre.is_some() && negative {
        col += 1;
        len -= 1;
        negative = false;
    }

    let mut subtract = delta < 0;
    let amount = delta.unsigned_abs();
    if negative {
        subtract = !subtract;
    }
    let oldn = parsed.value;
    let mut n = if subtract {
        oldn.wrapping_sub(amount)
    } else {
        oldn.wrapping_add(amount)
    };
    if parsed.pre.is_none() {
        // Decimal wraps through zero into the opposite sign; the prefixed
        // formats just wrap modulo 2^64 (`0x0<C-x>` → `0xffffffffffffffff`).
        if subtract {
            if n > oldn {
                n = 1u64.wrapping_add(!n);
                negative = !negative;
            }
        } else if n < oldn {
            n = !n;
            negative = !negative;
        }
        if n == 0 {
            negative = false;
        }
    }
    if parsed.overflow {
        // Vim leaves a literal too big for u64 saturated at u64::MAX.
        n = u64::MAX;
        negative = false;
    }

    // Take the case of the *last* alphabetic character in the literal — that
    // is how Vim decides `0xaB` → `0xAC` but `0xAb` → `0xac`.
    let mut hexupper = false;
    for c in chars.iter().skip(col).take(len) {
        if c.is_ascii_alphabetic() {
            hexupper = c.is_ascii_uppercase();
        }
    }

    let mut lead = String::new();
    let mut width = len;
    if negative {
        lead.push('-');
    }
    if let Some(p) = parsed.pre {
        lead.push('0');
        width -= 1;
        if p != '0' {
            lead.push(p);
            width -= 1;
        }
    }
    let digits = match parsed.pre {
        Some('b') | Some('B') => format!("{n:b}"),
        Some('0') => format!("{n:o}"),
        Some('x') | Some('X') if hexupper => format!("{n:X}"),
        Some('x') | Some('X') => format!("{n:x}"),
        _ => format!("{n}"),
    };
    width = width.saturating_sub(digits.chars().count());
    // Keep the total width by re-padding with zeros, unless that would make
    // the result look like an octal literal when 'nrformats' includes octal.
    if firstdigit == '0' && !(nf.oct && parsed.pre.is_none()) {
        for _ in 0..width {
            lead.push('0');
        }
    }
    lead.push_str(&digits);
    Some((col, len, lead))
}
