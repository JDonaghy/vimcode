//! Core comment toggling: language table, two-pass algorithm, and override resolution.
//!
//! This module provides a unified comment toggle feature that supports 46+ languages
//! with line comments, block comments, and extensible overrides via plugins and
//! extension manifests.

use std::collections::HashMap;

// ─── Comment style types ──────────────────────────────────────────────────────

/// Static comment style for built-in language table entries.
pub struct CommentStyle {
    pub line: &'static str,
    pub block_open: &'static str,
    pub block_close: &'static str,
}

/// Owned comment style, used for runtime overrides from plugins/extensions.
#[derive(Debug, Clone, Default)]
pub struct CommentStyleOwned {
    pub line: String,
    pub block_open: String,
    pub block_close: String,
}

// ─── Built-in language table ──────────────────────────────────────────────────

/// Returns the built-in comment style for a given LSP language ID.
pub fn comment_style_for_language(lang_id: &str) -> Option<&'static CommentStyle> {
    // Sorted by comment prefix for readability.
    Some(match lang_id {
        // `//` languages
        "rust" | "go" | "c" | "cpp" | "csharp" | "java" | "javascript" | "typescript"
        | "typescriptreact" | "javascriptreact" | "php" | "swift" | "kotlin" | "scala" | "dart"
        | "jsonc" | "zig" | "v" => {
            static S: CommentStyle = CommentStyle {
                line: "//",
                block_open: "/*",
                block_close: "*/",
            };
            &S
        }
        // `#` languages
        "python" | "ruby" | "shellscript" | "bash" | "yaml" | "toml" | "dockerfile" | "perl"
        | "r" | "makefile" | "cmake" | "nix" | "elixir" | "julia" | "fish" | "conf"
        | "gitignore" | "terraform" => {
            static S: CommentStyle = CommentStyle {
                line: "#",
                block_open: "",
                block_close: "",
            };
            &S
        }
        // `--` languages
        "lua" | "sql" | "haskell" | "elm" | "ada" => {
            static S: CommentStyle = CommentStyle {
                line: "--",
                block_open: "",
                block_close: "",
            };
            &S
        }
        // `%` languages
        "tex" | "latex" | "erlang" | "matlab" | "octave" => {
            static S: CommentStyle = CommentStyle {
                line: "%",
                block_open: "",
                block_close: "",
            };
            &S
        }
        // `;` languages
        "lisp" | "scheme" | "clojure" | "asm" => {
            static S: CommentStyle = CommentStyle {
                line: ";",
                block_open: "",
                block_close: "",
            };
            &S
        }
        // `"` languages
        "vim" => {
            static S: CommentStyle = CommentStyle {
                line: "\"",
                block_open: "",
                block_close: "",
            };
            &S
        }
        // Block-only languages (line prefix is the block open)
        "html" | "xml" | "svg" => {
            static S: CommentStyle = CommentStyle {
                line: "",
                block_open: "<!--",
                block_close: "-->",
            };
            &S
        }
        "css" | "scss" | "less" => {
            static S: CommentStyle = CommentStyle {
                line: "",
                block_open: "/*",
                block_close: "*/",
            };
            &S
        }
        _ => return None,
    })
}

// ─── Override resolution ──────────────────────────────────────────────────────

/// Resolve the comment style for a language, checking overrides first, then the
/// built-in table, then falling back to `#`.
pub fn resolve_comment_style(
    lang_id: &str,
    overrides: &HashMap<String, CommentStyleOwned>,
) -> CommentStyleOwned {
    // 1. Check runtime overrides (plugin + extension manifest)
    if let Some(ov) = overrides.get(lang_id) {
        return ov.clone();
    }
    // 2. Built-in table
    if let Some(s) = comment_style_for_language(lang_id) {
        return CommentStyleOwned {
            line: s.line.to_string(),
            block_open: s.block_open.to_string(),
            block_close: s.block_close.to_string(),
        };
    }
    // 3. Fallback
    CommentStyleOwned {
        line: "#".to_string(),
        block_open: String::new(),
        block_close: String::new(),
    }
}

// ─── Edit descriptor ──────────────────────────────────────────────────────────

/// Describes a single line edit produced by `compute_toggle_edits`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentEdit {
    /// 0-based index within the input `lines` slice.
    pub line_idx: usize,
    /// New full text for this line (without trailing newline).
    pub new_text: String,
}

// ─── Two-pass toggle algorithm ────────────────────────────────────────────────

/// Compute the edits needed to toggle comments on the given lines.
///
/// `lines` are the raw line strings (may include trailing newline).
/// Uses line comments when `line_prefix` is non-empty, otherwise falls back to
/// wrapping each line with `block_open` / `block_close`.
///
/// Returns `None` if there is no content to toggle (all blank).
pub fn compute_toggle_edits(
    lines: &[&str],
    line_prefix: &str,
    block_open: &str,
    block_close: &str,
) -> Option<Vec<CommentEdit>> {
    let use_block = line_prefix.is_empty() && !block_open.is_empty();
    let prefix = if use_block { block_open } else { line_prefix };
    let suffix = if use_block {
        format!(" {block_close}")
    } else {
        String::new()
    };
    // Pass 1: determine if all non-blank lines are already commented.
    let mut all_commented = true;
    let mut has_content = false;
    for line in lines {
        let trimmed = line
            .trim_start()
            .trim_end_matches('\n')
            .trim_end_matches('\r');
        if trimmed.is_empty() {
            continue;
        }
        has_content = true;
        if !is_commented(trimmed, prefix, &suffix) {
            all_commented = false;
            break;
        }
    }

    if !has_content {
        return None;
    }

    // Pass 2: build edits.
    let mut edits = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let raw = line.trim_end_matches('\n').trim_end_matches('\r');
        let ws_len = raw.len() - raw.trim_start().len();
        let indent = &raw[..ws_len];
        let rest = &raw[ws_len..];

        if rest.is_empty() {
            continue; // skip blank lines
        }

        let new_rest = if all_commented {
            uncomment_line(rest, prefix, &suffix)
        } else {
            format!("{prefix} {rest}{suffix}")
        };

        edits.push(CommentEdit {
            line_idx: i,
            new_text: format!("{indent}{new_rest}"),
        });
    }

    Some(edits)
}

/// Check if a trimmed (leading-whitespace-stripped) line is commented.
fn is_commented(trimmed: &str, prefix: &str, suffix: &str) -> bool {
    let prefix_space = format!("{prefix} ");
    let has_prefix = trimmed.starts_with(&prefix_space) || trimmed.starts_with(prefix);
    if !has_prefix {
        return false;
    }
    if suffix.is_empty() {
        return true;
    }
    // For block comments, also check suffix
    let suffix_trimmed = suffix.trim();
    trimmed.ends_with(suffix_trimmed)
}

/// Remove comment markers from a single line's content (after indent).
fn uncomment_line(rest: &str, prefix: &str, suffix: &str) -> String {
    let mut s = rest;
    let prefix_space = format!("{prefix} ");
    // Strip prefix
    if let Some(stripped) = s.strip_prefix(&prefix_space) {
        s = stripped;
    } else if let Some(stripped) = s.strip_prefix(prefix) {
        s = stripped;
    }
    // Strip suffix
    if !suffix.is_empty() {
        let suffix_trimmed = suffix.trim();
        if let Some(stripped) = s.strip_suffix(suffix_trimmed) {
            s = stripped;
            // Also strip trailing space before suffix
            s = s.trim_end_matches(' ');
        }
    }
    s.to_string()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_table_rust() {
        let s = comment_style_for_language("rust").unwrap();
        assert_eq!(s.line, "//");
        assert_eq!(s.block_open, "/*");
    }

    #[test]
    fn built_in_table_python() {
        let s = comment_style_for_language("python").unwrap();
        assert_eq!(s.line, "#");
    }

    #[test]
    fn built_in_table_html() {
        let s = comment_style_for_language("html").unwrap();
        assert_eq!(s.line, "");
        assert_eq!(s.block_open, "<!--");
        assert_eq!(s.block_close, "-->");
    }

    #[test]
    fn built_in_table_css() {
        let s = comment_style_for_language("css").unwrap();
        assert_eq!(s.line, "");
        assert_eq!(s.block_open, "/*");
        assert_eq!(s.block_close, "*/");
    }

    #[test]
    fn built_in_table_unknown_returns_none() {
        assert!(comment_style_for_language("brainfuck").is_none());
    }

    #[test]
    fn resolve_uses_override() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "rust".to_string(),
            CommentStyleOwned {
                line: "##".to_string(),
                block_open: String::new(),
                block_close: String::new(),
            },
        );
        let s = resolve_comment_style("rust", &overrides);
        assert_eq!(s.line, "##");
    }

    #[test]
    fn resolve_falls_back_to_hash() {
        let s = resolve_comment_style("unknown_lang", &HashMap::new());
        assert_eq!(s.line, "#");
    }

    #[test]
    fn toggle_comment_single_line() {
        let lines = vec!["    let x = 1;\n"];
        let edits = compute_toggle_edits(&lines, "//", "/*", "*/").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "    // let x = 1;");
    }

    #[test]
    fn toggle_uncomment_single_line() {
        let lines = vec!["    // let x = 1;\n"];
        let edits = compute_toggle_edits(&lines, "//", "/*", "*/").unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "    let x = 1;");
    }

    #[test]
    fn toggle_comment_mixed_range() {
        let lines = vec!["  foo\n", "  // bar\n", "  baz\n"];
        let edits = compute_toggle_edits(&lines, "//", "", "").unwrap();
        // Not all commented, so should add comments
        assert_eq!(edits.len(), 3);
        assert_eq!(edits[0].new_text, "  // foo");
        assert_eq!(edits[1].new_text, "  // // bar");
        assert_eq!(edits[2].new_text, "  // baz");
    }

    #[test]
    fn toggle_skips_blank_lines() {
        let lines = vec!["  foo\n", "\n", "  bar\n"];
        let edits = compute_toggle_edits(&lines, "#", "", "").unwrap();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].line_idx, 0);
        assert_eq!(edits[1].line_idx, 2);
    }

    #[test]
    fn toggle_all_blank_returns_none() {
        let lines = vec!["\n", "   \n", "\n"];
        assert!(compute_toggle_edits(&lines, "//", "", "").is_none());
    }

    #[test]
    fn toggle_block_comment_html() {
        let lines = vec!["  <div>\n"];
        let edits = compute_toggle_edits(&lines, "", "<!--", "-->").unwrap();
        assert_eq!(edits[0].new_text, "  <!-- <div> -->");
    }

    #[test]
    fn toggle_block_uncomment_html() {
        let lines = vec!["  <!-- <div> -->\n"];
        let edits = compute_toggle_edits(&lines, "", "<!--", "-->").unwrap();
        assert_eq!(edits[0].new_text, "  <div>");
    }

    #[test]
    fn toggle_block_comment_css() {
        let lines = vec!["  color: red;\n"];
        let edits = compute_toggle_edits(&lines, "", "/*", "*/").unwrap();
        assert_eq!(edits[0].new_text, "  /* color: red; */");
    }

    #[test]
    fn toggle_block_uncomment_css() {
        let lines = vec!["  /* color: red; */\n"];
        let edits = compute_toggle_edits(&lines, "", "/*", "*/").unwrap();
        assert_eq!(edits[0].new_text, "  color: red;");
    }

    #[test]
    fn uncomment_no_space_after_prefix() {
        let lines = vec!["//no space\n"];
        let edits = compute_toggle_edits(&lines, "//", "", "").unwrap();
        assert_eq!(edits[0].new_text, "no space");
    }

    #[test]
    fn python_comment_toggle() {
        let lines = vec!["    x = 1\n", "    y = 2\n"];
        let edits = compute_toggle_edits(&lines, "#", "", "").unwrap();
        assert_eq!(edits[0].new_text, "    # x = 1");
        assert_eq!(edits[1].new_text, "    # y = 2");
    }

    #[test]
    fn lua_comment_toggle() {
        let lines = vec!["local x = 1\n"];
        let edits = compute_toggle_edits(&lines, "--", "", "").unwrap();
        assert_eq!(edits[0].new_text, "-- local x = 1");
    }
}
