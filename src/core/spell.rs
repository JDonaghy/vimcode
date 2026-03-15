//! Spell-checking module using spellbook (pure-Rust Hunspell parser).
//!
//! Bundled en_US dictionary compiled into the binary.  User dictionary
//! at `~/.config/vimcode/user.dic` (one word per line).

use super::syntax::SyntaxLanguage;
use std::path::PathBuf;

/// A misspelled word with its byte and char position within a line.
#[derive(Debug, Clone)]
pub struct SpellError {
    /// Char (not byte) column of the start of the misspelled word.
    pub start_col: usize,
    /// Char column one past the end of the misspelled word.
    pub end_col: usize,
    /// The misspelled word itself.
    pub word: String,
}

/// Manages the spell-checking dictionary.
pub struct SpellChecker {
    dict: spellbook::Dictionary,
    user_words: Vec<String>,
}

// Bundled dictionaries compiled into the binary.
const BUNDLED_AFF: &str = include_str!("../../dictionaries/en_US.aff");
const BUNDLED_DIC: &str = include_str!("../../dictionaries/en_US.dic");

impl SpellChecker {
    /// Create a new spell checker with the bundled en_US dictionary
    /// and any words from the user dictionary file.
    pub fn new() -> Option<Self> {
        let dict = spellbook::Dictionary::new(BUNDLED_AFF, BUNDLED_DIC).ok()?;
        let user_words = load_user_dict_words();
        Some(SpellChecker { dict, user_words })
    }

    /// Returns true if the word is correctly spelled.
    pub fn check_word(&self, word: &str) -> bool {
        // Skip words that are: single char, all-uppercase, contain digits
        if word.len() <= 1 {
            return true;
        }
        if word.chars().all(|c| c.is_ascii_uppercase()) {
            return true;
        }
        if word.chars().any(|c| c.is_ascii_digit()) {
            return true;
        }
        // Check user dictionary first
        if self.user_words.iter().any(|w| w.eq_ignore_ascii_case(word)) {
            return true;
        }
        self.dict.check(word)
    }

    /// Add a word to the user dictionary and persist to disk.
    pub fn add_to_user_dict(&mut self, word: &str) {
        let w = word.to_string();
        if !self.user_words.contains(&w) {
            self.user_words.push(w);
            save_user_dict_words(&self.user_words);
        }
    }

    /// Return spelling suggestions for a misspelled word.
    pub fn suggest(&self, word: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.dict.suggest(word, &mut out);
        out
    }

    /// Remove a word from the user dictionary.
    pub fn remove_from_user_dict(&mut self, word: &str) {
        self.user_words.retain(|w| !w.eq_ignore_ascii_case(word));
        save_user_dict_words(&self.user_words);
    }
}

/// Extract the user dictionary path.
fn user_dict_path() -> PathBuf {
    super::paths::vimcode_config_dir().join("user.dic")
}

/// Load user dictionary words from disk.
fn load_user_dict_words() -> Vec<String> {
    let path = user_dict_path();
    match std::fs::read_to_string(&path) {
        Ok(contents) => contents
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.trim().to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Save user dictionary words to disk.
fn save_user_dict_words(words: &[String]) {
    #[cfg(test)]
    {
        let _ = words;
        return;
    }
    #[cfg(not(test))]
    {
        let path = user_dict_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = words.join("\n");
        let _ = std::fs::write(&path, content);
    }
}

/// Check a single line for spelling errors.
///
/// `syntax_lang` controls scope-aware checking:
/// - `None` → plain text, check all words.
/// - `Some(Latex)` → **inverted**: check all words EXCEPT those inside
///   `command_name`, `inline_formula`, `math_environment`, or
///   `displayed_equation` scopes (LaTeX commands & math are not prose).
/// - `Some(_)` → standard code mode: only check words inside `comment`
///   or `string` scopes.
///
/// `highlights` are `(start_byte, end_byte, scope)` relative to the
/// full buffer.  `line_start_byte` is the byte offset of this line
/// within the buffer.
pub fn check_line(
    checker: &SpellChecker,
    line: &str,
    highlights: &[(usize, usize, String)],
    line_start_byte: usize,
    syntax_lang: Option<SyntaxLanguage>,
) -> Vec<SpellError> {
    let mut errors = Vec::new();
    // Extract words: contiguous runs of alphabetic + apostrophe chars
    let mut chars: Vec<(usize, char)> = line.char_indices().collect();
    // Add sentinel
    chars.push((line.len(), '\0'));

    let mut word_start: Option<usize> = None;
    let mut word_start_col: usize = 0;

    for (col, &(byte_idx, ch)) in chars.iter().enumerate() {
        let is_word_char = ch.is_alphabetic() || ch == '\'';
        if is_word_char {
            if word_start.is_none() {
                word_start = Some(byte_idx);
                word_start_col = col;
            }
        } else if let Some(ws) = word_start {
            let word = &line[ws..byte_idx];
            // Strip leading/trailing apostrophes
            let trimmed = word.trim_matches('\'');
            if trimmed.len() >= 2 {
                let trim_offset = word.find(trimmed).unwrap_or(0);
                let trimmed_start_byte = ws + trim_offset;
                let trimmed_end_byte = trimmed_start_byte + trimmed.len();
                let trimmed_start_col = word_start_col + word[..trim_offset].chars().count();
                let trimmed_end_col = trimmed_start_col + trimmed.chars().count();

                let abs_start = line_start_byte + trimmed_start_byte;
                let abs_end = line_start_byte + trimmed_end_byte;

                let should_check = match syntax_lang {
                    None => true,
                    Some(SyntaxLanguage::Latex) => {
                        // Inverted: check everything EXCEPT commands/math
                        !is_in_latex_command_or_math(highlights, abs_start, abs_end)
                    }
                    Some(_) => is_in_comment_or_string(highlights, abs_start, abs_end),
                };

                if should_check && !checker.check_word(trimmed) {
                    errors.push(SpellError {
                        start_col: trimmed_start_col,
                        end_col: trimmed_end_col,
                        word: trimmed.to_string(),
                    });
                }
            }
            word_start = None;
        }
    }
    errors
}

/// Returns true if any byte in `[start, end)` overlaps a LaTeX command or math scope.
fn is_in_latex_command_or_math(
    highlights: &[(usize, usize, String)],
    start: usize,
    end: usize,
) -> bool {
    for (hs, he, scope) in highlights {
        if *he <= start {
            continue;
        }
        if *hs >= end {
            continue;
        }
        // keyword = command_name, type = inline_formula/math_environment/displayed_equation
        if scope == "keyword" || scope == "type" {
            return true;
        }
    }
    false
}

/// Returns true if any byte in `[start, end)` overlaps a comment or string scope.
fn is_in_comment_or_string(
    highlights: &[(usize, usize, String)],
    start: usize,
    end: usize,
) -> bool {
    for (hs, he, scope) in highlights {
        if *he <= start {
            continue;
        }
        if *hs >= end {
            continue;
        }
        // Overlapping range — check scope
        if scope.contains("comment") || scope.contains("string") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker() -> SpellChecker {
        SpellChecker::new().expect("Failed to load bundled dictionary")
    }

    #[test]
    fn test_check_word_correct() {
        let c = checker();
        assert!(c.check_word("hello"));
        assert!(c.check_word("world"));
        assert!(c.check_word("the"));
    }

    #[test]
    fn test_check_word_incorrect() {
        let c = checker();
        assert!(!c.check_word("helo"));
        assert!(!c.check_word("wrld"));
        assert!(!c.check_word("speling"));
    }

    #[test]
    fn test_skip_single_char() {
        let c = checker();
        assert!(c.check_word("x"));
        assert!(c.check_word("Q"));
    }

    #[test]
    fn test_skip_all_caps() {
        let c = checker();
        assert!(c.check_word("HTTP"));
        assert!(c.check_word("API"));
        assert!(c.check_word("JSON"));
    }

    #[test]
    fn test_skip_with_digits() {
        let c = checker();
        assert!(c.check_word("abc123"));
        assert!(c.check_word("h2o"));
    }

    #[test]
    fn test_user_dict() {
        let mut c = checker();
        assert!(!c.check_word("vimcode"));
        c.add_to_user_dict("vimcode");
        assert!(c.check_word("vimcode"));
        c.remove_from_user_dict("vimcode");
        assert!(!c.check_word("vimcode"));
    }

    #[test]
    fn test_check_line_plain_text() {
        let c = checker();
        let line = "The quik brown fox";
        let errors = check_line(&c, line, &[], 0, None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].word, "quik");
        assert_eq!(errors[0].start_col, 4);
        assert_eq!(errors[0].end_col, 8);
    }

    #[test]
    fn test_check_line_no_errors() {
        let c = checker();
        let line = "The quick brown fox";
        let errors = check_line(&c, line, &[], 0, None);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_check_line_syntax_aware() {
        let c = checker();
        // "helo" at bytes 0..4, comment scope covers it
        let line = "helo world";
        let highlights = vec![(0, 4, "comment".to_string())];
        let errors = check_line(&c, line, &highlights, 0, Some(SyntaxLanguage::Rust));
        // "helo" is in comment scope → checked → flagged
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].word, "helo");

        // "world" at bytes 5..10 is NOT in comment/string → not checked
        // (verified by the fact we only got 1 error)
    }

    #[test]
    fn test_check_line_syntax_aware_skips_code() {
        let c = checker();
        let line = "fn helo() {}";
        // No comment/string scopes — everything is code
        let highlights = vec![(0, 2, "keyword".to_string())];
        let errors = check_line(&c, line, &highlights, 0, Some(SyntaxLanguage::Rust));
        assert!(errors.is_empty()); // "helo" not in comment/string, skipped
    }

    #[test]
    fn test_check_line_latex_checks_prose() {
        let c = checker();
        // In LaTeX, prose words outside commands/math should be checked
        let line = "This has a speling error";
        let errors = check_line(&c, line, &[], 0, Some(SyntaxLanguage::Latex));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].word, "speling");
    }

    #[test]
    fn test_check_line_latex_skips_commands() {
        let c = checker();
        // LaTeX command "\documentclass" — the command_name maps to "keyword"
        let line = "\\documentclass article";
        // "documentclass" at bytes 1..14 is keyword scope
        let highlights = vec![(1, 14, "keyword".to_string())];
        let errors = check_line(&c, line, &highlights, 0, Some(SyntaxLanguage::Latex));
        // "documentclass" is in keyword scope → skipped in LaTeX mode
        // "article" is not in any scope → checked (it's a valid English word)
        assert!(errors.is_empty());
    }

    #[test]
    fn test_check_line_latex_skips_math() {
        let c = checker();
        // Math formula content maps to "type" scope
        let line = "See $\\alpha + \\beta$ here";
        // inline_formula bytes covering the math part
        let highlights = vec![(4, 20, "type".to_string())];
        let errors = check_line(&c, line, &highlights, 0, Some(SyntaxLanguage::Latex));
        // "alpha" and "beta" are in type scope → skipped
        // "See" and "here" are valid English words
        assert!(errors.is_empty());
    }

    #[test]
    fn test_apostrophe_handling() {
        let c = checker();
        let line = "don't won't can't";
        let errors = check_line(&c, line, &[], 0, None);
        assert!(errors.is_empty());
    }
}
