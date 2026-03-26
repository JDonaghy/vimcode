use super::*;
use crate::core::spell;
use crate::core::syntax::SyntaxLanguage;

impl Engine {
    // ─── Spell checking ──────────────────────────────────────────────────

    /// Ensure the spell checker is initialized (called when `:set spell`).
    pub fn ensure_spell_checker(&mut self) {
        if self.spell_checker.is_none() && self.settings.spell {
            self.spell_checker = spell::SpellChecker::new();
        }
    }

    /// Jump to the next misspelled word after the cursor.
    pub fn jump_next_spell_error(&mut self) {
        if !self.settings.spell {
            self.message = "Spell checking is off (use :set spell)".to_string();
            return;
        }
        self.ensure_spell_checker();
        let checker = match self.spell_checker.take() {
            Some(c) => c,
            None => return,
        };
        let line_count = self.buffer().content.len_lines();
        let cur_line = self.cursor().line;
        let cur_col = self.cursor().col;
        let syntax_lang = self
            .active_buffer_state()
            .file_path
            .as_ref()
            .and_then(|p| p.to_str())
            .and_then(SyntaxLanguage::from_path);

        let mut found = None;
        for offset in 0..line_count {
            let li = (cur_line + offset) % line_count;
            let line_str: String = self.buffer().content.line(li).chars().collect();
            let line_start_byte = self.buffer().content.line_to_byte(li);
            let highlights = &self.active_buffer_state().highlights;
            let errors = spell::check_line(
                &checker,
                &line_str,
                highlights,
                line_start_byte,
                syntax_lang,
            );
            for e in &errors {
                if li == cur_line && e.start_col <= cur_col {
                    continue;
                }
                found = Some((li, e.start_col, e.word.clone()));
                break;
            }
            if found.is_some() {
                break;
            }
        }
        self.spell_checker = Some(checker);
        if let Some((line, col, word)) = found {
            self.view_mut().cursor.line = line;
            self.view_mut().cursor.col = col;
            self.ensure_cursor_visible();
            self.message = format!("Misspelled: {}", word);
        } else {
            self.message = "No spelling errors found".to_string();
        }
    }

    /// Jump to the previous misspelled word before the cursor.
    pub fn jump_prev_spell_error(&mut self) {
        if !self.settings.spell {
            self.message = "Spell checking is off (use :set spell)".to_string();
            return;
        }
        self.ensure_spell_checker();
        let checker = match self.spell_checker.take() {
            Some(c) => c,
            None => return,
        };
        let line_count = self.buffer().content.len_lines();
        let cur_line = self.cursor().line;
        let cur_col = self.cursor().col;
        let syntax_lang = self
            .active_buffer_state()
            .file_path
            .as_ref()
            .and_then(|p| p.to_str())
            .and_then(SyntaxLanguage::from_path);

        let mut found = None;
        for offset in 0..line_count {
            let li = (cur_line + line_count - offset) % line_count;
            let line_str: String = self.buffer().content.line(li).chars().collect();
            let line_start_byte = self.buffer().content.line_to_byte(li);
            let highlights = &self.active_buffer_state().highlights;
            let errors = spell::check_line(
                &checker,
                &line_str,
                highlights,
                line_start_byte,
                syntax_lang,
            );
            for e in errors.iter().rev() {
                if li == cur_line && e.start_col >= cur_col {
                    continue;
                }
                found = Some((li, e.start_col, e.word.clone()));
                break;
            }
            if found.is_some() {
                break;
            }
        }
        self.spell_checker = Some(checker);
        if let Some((line, col, word)) = found {
            self.view_mut().cursor.line = line;
            self.view_mut().cursor.col = col;
            self.ensure_cursor_visible();
            self.message = format!("Misspelled: {}", word);
        } else {
            self.message = "No spelling errors found".to_string();
        }
    }

    /// Show spell suggestions for the word under the cursor (z=).
    pub fn spell_show_suggestions(&mut self) {
        if !self.settings.spell {
            self.message = "Spell checking is off (use :set spell)".to_string();
            return;
        }
        let word = match self.word_under_cursor() {
            Some(w) => w,
            None => {
                self.message = "No word under cursor".to_string();
                return;
            }
        };
        self.ensure_spell_checker();
        if let Some(ref checker) = self.spell_checker {
            if checker.check_word(&word) {
                self.message = format!("'{}' is correctly spelled", word);
            } else {
                let suggestions = checker.suggest(&word);
                if suggestions.is_empty() {
                    self.message = format!("'{}' is misspelled — no suggestions (zg to add)", word);
                } else {
                    // Build numbered list like Vim: 1-9, a-z for quick selection
                    let labels: Vec<char> = "123456789abcdefghijklmnopqrstuvwxyz".chars().collect();
                    let count = suggestions.len().min(labels.len());
                    let mut parts: Vec<String> = Vec::new();
                    for i in 0..count {
                        parts.push(format!("[{}]{}", labels[i], suggestions[i]));
                    }
                    self.message = format!("\"{}\" -> {} (Esc=cancel)", word, parts.join(" "));
                    self.spell_suggestions = Some((
                        word,
                        suggestions.into_iter().take(count).collect(),
                        String::new(),
                    ));
                }
            }
        }
    }

    /// Handle a key press while spell suggestions are pending.
    /// Returns true if the key was consumed.
    pub fn handle_spell_suggestion_key(&mut self, key_name: &str, unicode: Option<char>) -> bool {
        let (word, suggestions, _input) = match self.spell_suggestions.take() {
            Some(s) => s,
            None => return false,
        };

        // Escape cancels
        if key_name == "Escape" {
            self.message = String::new();
            return true;
        }

        // Direct single-key selection: 1-9, a-z
        if let Some(ch) = unicode {
            let labels: Vec<char> = "123456789abcdefghijklmnopqrstuvwxyz".chars().collect();
            if let Some(idx) = labels.iter().position(|&c| c == ch) {
                if idx < suggestions.len() {
                    self.spell_replace_word(&word, &suggestions[idx]);
                    self.message = format!("Changed \"{}\" to \"{}\"", word, suggestions[idx]);
                    return true;
                }
            }
        }

        // Any other key cancels
        self.message = String::new();
        true
    }

    /// Replace the word under the cursor with a replacement.
    fn spell_replace_word(&mut self, old_word: &str, new_word: &str) {
        let cursor_line = self.cursor().line;
        let cursor_col = self.cursor().col;
        let line_str: String = self.buffer().content.line(cursor_line).chars().collect();

        // Find the word boundaries around the cursor
        let chars: Vec<(usize, char)> = line_str.char_indices().collect();
        let mut word_start = cursor_col;
        let mut word_end = cursor_col;

        // Walk back to find word start
        while word_start > 0 {
            let ch = chars.get(word_start - 1).map(|c| c.1).unwrap_or(' ');
            if ch.is_alphabetic() || ch == '\'' {
                word_start -= 1;
            } else {
                break;
            }
        }
        // Walk forward to find word end
        while word_end < chars.len() {
            let ch = chars[word_end].1;
            if ch.is_alphabetic() || ch == '\'' {
                word_end += 1;
            } else {
                break;
            }
        }

        // Get the actual word at this position
        let actual_word: String = chars[word_start..word_end].iter().map(|c| c.1).collect();
        if actual_word.trim_matches('\'') != old_word {
            return; // Word changed since z= was pressed
        }

        // Calculate char positions for replacement
        let line_char_offset = self.buffer().content.line_to_char(cursor_line);
        let start_char = line_char_offset + word_start;
        let end_char = line_char_offset + word_end;

        self.start_undo_group();
        self.delete_with_undo(start_char, end_char);
        self.insert_with_undo(start_char, new_word);
        self.finish_undo_group();
        self.set_dirty(true);
        self.update_syntax();
        let active_id = self.active_buffer_id();
        self.lsp_dirty_buffers.insert(active_id, true);
        self.swap_mark_dirty();
        self.view_mut().cursor.col = word_start;
    }

    /// Add the word under the cursor to the user dictionary (zg).
    pub fn spell_add_good_word(&mut self) {
        let word = match self.word_under_cursor() {
            Some(w) => w,
            None => {
                self.message = "No word under cursor".to_string();
                return;
            }
        };
        self.ensure_spell_checker();
        if let Some(ref mut checker) = self.spell_checker {
            checker.add_to_user_dict(&word);
            self.message = format!("Added '{}' to user dictionary", word);
        }
    }

    /// Mark the word under the cursor as wrong (zw).
    pub fn spell_mark_wrong(&mut self) {
        let word = match self.word_under_cursor() {
            Some(w) => w,
            None => {
                self.message = "No word under cursor".to_string();
                return;
            }
        };
        self.ensure_spell_checker();
        if let Some(ref mut checker) = self.spell_checker {
            checker.remove_from_user_dict(&word);
            self.message = format!("Removed '{}' from user dictionary", word);
        }
    }
}
