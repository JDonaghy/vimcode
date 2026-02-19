//! Project-wide file search with regex, case-sensitivity, and whole-word toggles.
//!
//! Uses the `ignore` crate (same walker as ripgrep) to respect `.gitignore`
//! and skip binary files.  Entirely in `core/` — no UI dependencies.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Maximum number of results returned to prevent memory issues on huge repos.
const MAX_RESULTS: usize = 10_000;

/// A single match found during a project search.
#[derive(Debug, Clone)]
pub struct ProjectMatch {
    /// Absolute path to the file.
    pub file: PathBuf,
    /// 0-indexed line number within the file.
    pub line: usize,
    /// Byte offset of the match start within the line.
    /// Reserved for future highlight support.
    #[allow(dead_code)]
    pub col: usize,
    /// Full text of the line (trimmed to avoid rendering issues).
    pub line_text: String,
}

/// Search-mode toggles (mirrors VS Code's Aa / Ab| / .* buttons).
#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    /// When `true`, matching is case-sensitive.
    pub case_sensitive: bool,
    /// When `true`, the query matches whole words only (`\b...\b`).
    pub whole_word: bool,
    /// When `true`, the query is interpreted as a regular expression.
    pub use_regex: bool,
}

/// Error returned when the user-supplied regex is invalid.
#[derive(Debug, Clone)]
pub struct SearchError(pub String);

/// Result of a project-wide replace operation.
#[derive(Debug, Clone)]
pub struct ReplaceResult {
    /// Total number of individual replacements made across all files.
    pub replacement_count: usize,
    /// Number of files that were modified.
    pub file_count: usize,
    /// Files that were skipped (e.g. dirty buffers).
    pub skipped_files: Vec<PathBuf>,
    /// Files that were actually written to.
    pub modified_files: Vec<PathBuf>,
}

/// Build a compiled regex from the user query and search options.
///
/// Shared by `search_in_project` and `replace_in_project`.
fn build_search_regex(query: &str, options: &SearchOptions) -> Result<regex::Regex, SearchError> {
    let escaped = if options.use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let with_boundary = if options.whole_word {
        format!(r"\b{}\b", escaped)
    } else {
        escaped
    };
    let full_pattern = if options.case_sensitive {
        with_boundary
    } else {
        format!("(?i){}", with_boundary)
    };
    regex::Regex::new(&full_pattern).map_err(|e| SearchError(e.to_string()))
}

/// Search all text files under `root` for `query` using the given `options`.
///
/// - Respects `.gitignore` rules via the `ignore` crate.
/// - Binary files (non-UTF-8) are silently skipped.
/// - Hidden files/directories are skipped by default (ignore crate behaviour).
/// - Results are sorted by file path, then line number.
/// - At most `MAX_RESULTS` matches are returned.
/// - Returns `Err(SearchError)` if `use_regex` is true and the pattern is invalid.
pub fn search_in_project(
    root: &Path,
    query: &str,
    options: &SearchOptions,
) -> Result<Vec<ProjectMatch>, SearchError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let re = build_search_regex(query, options)?;

    let mut results: Vec<ProjectMatch> = Vec::new();

    let walker = ignore::WalkBuilder::new(root)
        .hidden(true) // skip hidden files/dirs
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Only process files, not directories.
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        let path = entry.path();
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // binary or unreadable — skip
        };

        for (line_idx, line_text) in content.lines().enumerate() {
            if let Some(m) = re.find(line_text) {
                results.push(ProjectMatch {
                    file: path.to_path_buf(),
                    line: line_idx,
                    col: m.start(),
                    line_text: line_text.to_string(),
                });
                if results.len() >= MAX_RESULTS {
                    results.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
                    return Ok(results);
                }
            }
        }
    }

    results.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    Ok(results)
}

/// Replace all occurrences of `query` with `replacement` across files under `root`.
///
/// - Respects `.gitignore` rules via the `ignore` crate.
/// - Files whose canonicalized path appears in `skip_paths` are skipped (reported in result).
/// - In literal mode (`use_regex=false`), `$` in `replacement` is treated literally.
/// - In regex mode (`use_regex=true`), `$1`, `$2` etc. expand to capture groups.
/// - Only writes back files whose content actually changed.
pub fn replace_in_project(
    root: &Path,
    query: &str,
    replacement: &str,
    options: &SearchOptions,
    skip_paths: &HashSet<PathBuf>,
) -> Result<ReplaceResult, SearchError> {
    if query.is_empty() {
        return Ok(ReplaceResult {
            replacement_count: 0,
            file_count: 0,
            skipped_files: Vec::new(),
            modified_files: Vec::new(),
        });
    }

    let re = build_search_regex(query, options)?;

    let mut result = ReplaceResult {
        replacement_count: 0,
        file_count: 0,
        skipped_files: Vec::new(),
        modified_files: Vec::new(),
    };

    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        let path = entry.path().to_path_buf();

        // Check skip_paths using canonical path for reliable comparison.
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if skip_paths.contains(&canonical) {
            // Only report as skipped if the file actually has matches.
            if let Ok(content) = fs::read_to_string(&path) {
                if re.is_match(&content) {
                    result.skipped_files.push(path);
                }
            }
            continue;
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let match_count = re.find_iter(&content).count();
        if match_count == 0 {
            continue;
        }

        // In literal mode, prevent $1 etc. from being interpreted as backreferences.
        let new_content = if options.use_regex {
            re.replace_all(&content, replacement).into_owned()
        } else {
            re.replace_all(&content, regex::NoExpand(replacement))
                .into_owned()
        };

        if new_content != content && fs::write(&path, &new_content).is_ok() {
            result.replacement_count += match_count;
            result.file_count += 1;
            result.modified_files.push(path);
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_temp_project(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vimcode_psearch_{}", test_name));
        // Clean up from any previous run
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // file1.txt — has "hello world" on line 0
        let mut f1 = fs::File::create(dir.join("file1.txt")).unwrap();
        writeln!(f1, "hello world").unwrap();
        writeln!(f1, "no match here").unwrap();
        writeln!(f1, "HELLO again").unwrap(); // case-insensitive match

        // subdir/file2.txt — has "Hello" on line 1
        fs::create_dir_all(dir.join("sub")).unwrap();
        let mut f2 = fs::File::create(dir.join("sub/file2.txt")).unwrap();
        writeln!(f2, "nothing").unwrap();
        writeln!(f2, "Hello from sub").unwrap();

        // .hidden/secret.txt — should be skipped
        fs::create_dir_all(dir.join(".hidden")).unwrap();
        let mut fh = fs::File::create(dir.join(".hidden/secret.txt")).unwrap();
        writeln!(fh, "hello hidden").unwrap();

        dir
    }

    #[test]
    fn test_empty_query_returns_nothing() {
        let dir = make_temp_project("empty_query");
        let results = search_in_project(&dir, "", &SearchOptions::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_case_insensitive_match() {
        let dir = make_temp_project("case_insensitive");
        let results = search_in_project(&dir, "hello", &SearchOptions::default()).unwrap();
        // file1.txt lines 0 and 2, sub/file2.txt line 1 — hidden excluded
        assert_eq!(results.len(), 3);
        // Sorted by file path then line
        assert_eq!(results[0].line, 0);
        assert_eq!(results[0].line_text, "hello world");
        assert_eq!(results[1].line, 2);
        assert_eq!(results[1].line_text, "HELLO again");
        assert_eq!(results[2].line, 1);
        assert_eq!(results[2].line_text, "Hello from sub");
    }

    #[test]
    fn test_no_results() {
        let dir = make_temp_project("no_results");
        let results = search_in_project(&dir, "zzznomatch", &SearchOptions::default()).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_hidden_dirs_skipped() {
        let dir = make_temp_project("hidden_dirs");
        let results = search_in_project(&dir, "hidden", &SearchOptions::default()).unwrap();
        assert!(
            results.is_empty(),
            "hidden directory should be skipped, got: {:?}",
            results
        );
    }

    #[test]
    fn test_col_offset() {
        let dir = make_temp_project("col_offset");
        let results = search_in_project(&dir, "world", &SearchOptions::default()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].col, 6); // "hello " = 6 bytes
    }

    // ── New tests for SearchOptions ──────────────────────────────────────

    #[test]
    fn test_case_sensitive_search() {
        let dir = make_temp_project("case_sensitive");
        let opts = SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        let results = search_in_project(&dir, "hello", &opts).unwrap();
        // Only "hello world" matches — "HELLO again" and "Hello from sub" are excluded
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].line_text, "hello world");
    }

    #[test]
    fn test_whole_word_search() {
        let dir = make_temp_project("whole_word");
        // "hello" should NOT match "helloworld" (if it were present)
        // but SHOULD match "hello world" (word boundary)
        // Add a file with "helloworld" concatenated
        let mut f = fs::File::create(dir.join("concat.txt")).unwrap();
        writeln!(f, "helloworld joined").unwrap();
        writeln!(f, "hello world apart").unwrap();

        let opts = SearchOptions {
            whole_word: true,
            ..Default::default()
        };
        let results = search_in_project(&dir, "hello", &opts).unwrap();
        // "helloworld joined" should NOT match whole word
        for r in &results {
            assert!(
                !r.line_text.contains("helloworld"),
                "whole word should not match 'helloworld', got: {}",
                r.line_text
            );
        }
        // "hello world apart" should match
        assert!(
            results.iter().any(|r| r.line_text == "hello world apart"),
            "whole word should match 'hello world apart'"
        );
    }

    #[test]
    fn test_regex_search() {
        let dir = make_temp_project("regex_search");
        let opts = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        // Use regex pattern that matches "hello" followed by any whitespace + word
        let results = search_in_project(&dir, r"hello\s+\w+", &opts).unwrap();
        assert!(!results.is_empty(), "regex should find matches");
        // All results should contain "hello" followed by space(s) + word chars
        for r in &results {
            let lower = r.line_text.to_lowercase();
            assert!(
                lower.contains("hello ") || lower.contains("hello\t"),
                "regex match should contain hello + whitespace: {}",
                r.line_text
            );
        }
    }

    #[test]
    fn test_invalid_regex_returns_error() {
        let dir = make_temp_project("invalid_regex");
        let opts = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let result = search_in_project(&dir, "[bad", &opts);
        assert!(result.is_err(), "invalid regex should return Err");
        let err = result.unwrap_err();
        assert!(!err.0.is_empty(), "error message should be non-empty");
    }

    #[test]
    fn test_whole_word_regex_combo() {
        let dir = make_temp_project("word_regex");
        let mut f = fs::File::create(dir.join("combo.txt")).unwrap();
        writeln!(f, "testing test tested").unwrap();

        let opts = SearchOptions {
            use_regex: true,
            whole_word: true,
            ..Default::default()
        };
        let results = search_in_project(&dir, "test", &opts).unwrap();
        // "test" as whole word should match "test" but NOT "testing" or "tested"
        // The line "testing test tested" contains whole-word "test" so it matches
        assert!(
            results
                .iter()
                .any(|r| r.line_text.contains("testing test tested")),
            "should match line containing whole-word 'test'"
        );
    }

    #[test]
    fn test_gitignore_respected() {
        let dir = make_temp_project("gitignore");
        // Initialize a git repo so the ignore crate honours .gitignore
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .expect("git init");
        // Create a .gitignore that ignores "ignored_dir/"
        let mut gi = fs::File::create(dir.join(".gitignore")).unwrap();
        writeln!(gi, "ignored_dir/").unwrap();
        fs::create_dir_all(dir.join("ignored_dir")).unwrap();
        let mut fi = fs::File::create(dir.join("ignored_dir/data.txt")).unwrap();
        writeln!(fi, "hello from ignored").unwrap();

        let results =
            search_in_project(&dir, "hello from ignored", &SearchOptions::default()).unwrap();
        assert!(
            results.is_empty(),
            ".gitignore should exclude ignored_dir/, got: {:?}",
            results
        );
    }

    // ── Replace tests ────────────────────────────────────────────────────

    fn make_replace_project(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vimcode_preplace_{}", test_name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_replace_basic() {
        let dir = make_replace_project("basic");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "hello world").unwrap();
        writeln!(f, "hello again").unwrap();
        drop(f);

        let rr = replace_in_project(
            &dir,
            "hello",
            "hi",
            &SearchOptions::default(),
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(rr.replacement_count, 2);
        assert_eq!(rr.file_count, 1);
        assert!(rr.skipped_files.is_empty());
        let content = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(content.contains("hi world"));
        assert!(content.contains("hi again"));
        assert!(!content.contains("hello"));
    }

    #[test]
    fn test_replace_case_insensitive() {
        let dir = make_replace_project("case_insensitive");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "Hello World").unwrap();
        writeln!(f, "HELLO AGAIN").unwrap();
        drop(f);

        let rr = replace_in_project(
            &dir,
            "hello",
            "hi",
            &SearchOptions::default(), // case_sensitive=false
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(rr.replacement_count, 2);
        let content = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(content.contains("hi World"));
        assert!(content.contains("hi AGAIN"));
    }

    #[test]
    fn test_replace_whole_word() {
        let dir = make_replace_project("whole_word");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "helloworld hello").unwrap();
        drop(f);

        let opts = SearchOptions {
            whole_word: true,
            ..Default::default()
        };
        let rr = replace_in_project(&dir, "hello", "hi", &opts, &HashSet::new()).unwrap();
        assert_eq!(rr.replacement_count, 1);
        let content = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(content.contains("helloworld hi"));
    }

    #[test]
    fn test_replace_regex_capture_groups() {
        let dir = make_replace_project("regex_capture");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "foo_old bar_old").unwrap();
        drop(f);

        let opts = SearchOptions {
            use_regex: true,
            case_sensitive: true,
            ..Default::default()
        };
        let rr =
            replace_in_project(&dir, r"(\w+)_old", "${1}_new", &opts, &HashSet::new()).unwrap();
        assert_eq!(rr.replacement_count, 2);
        let content = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(content.contains("foo_new bar_new"));
    }

    #[test]
    fn test_replace_literal_dollar_sign() {
        let dir = make_replace_project("literal_dollar");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "price is 10").unwrap();
        drop(f);

        // In literal mode (use_regex=false), $1 in replacement should be literal.
        let rr = replace_in_project(
            &dir,
            "10",
            "$1.00",
            &SearchOptions::default(),
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(rr.replacement_count, 1);
        let content = fs::read_to_string(dir.join("a.txt")).unwrap();
        assert!(
            content.contains("$1.00"),
            "literal $1 should not be interpreted as backreference, got: {}",
            content
        );
    }

    #[test]
    fn test_replace_skip_dirty_files() {
        let dir = make_replace_project("skip_dirty");
        let path = dir.join("a.txt");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "hello world").unwrap();
        drop(f);

        let canonical = path.canonicalize().unwrap();
        let mut skip = HashSet::new();
        skip.insert(canonical);

        let rr = replace_in_project(&dir, "hello", "hi", &SearchOptions::default(), &skip).unwrap();
        assert_eq!(rr.replacement_count, 0);
        assert_eq!(rr.file_count, 0);
        assert_eq!(rr.skipped_files.len(), 1);
        // File should be unchanged
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("hello world"));
    }

    #[test]
    fn test_replace_empty_query() {
        let dir = make_replace_project("empty_query");
        let mut f = fs::File::create(dir.join("a.txt")).unwrap();
        writeln!(f, "hello").unwrap();
        drop(f);

        let rr =
            replace_in_project(&dir, "", "hi", &SearchOptions::default(), &HashSet::new()).unwrap();
        assert_eq!(rr.replacement_count, 0);
        assert_eq!(rr.file_count, 0);
    }

    #[test]
    fn test_replace_invalid_regex() {
        let dir = make_replace_project("invalid_regex");
        let opts = SearchOptions {
            use_regex: true,
            ..Default::default()
        };
        let result = replace_in_project(&dir, "[bad", "x", &opts, &HashSet::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_replace_gitignore_respected() {
        let dir = make_replace_project("gitignore");
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .expect("git init");
        let mut gi = fs::File::create(dir.join(".gitignore")).unwrap();
        writeln!(gi, "ignored/").unwrap();
        drop(gi);
        fs::create_dir_all(dir.join("ignored")).unwrap();
        let path = dir.join("ignored/data.txt");
        let mut fi = fs::File::create(&path).unwrap();
        writeln!(fi, "hello world").unwrap();
        drop(fi);

        let rr = replace_in_project(
            &dir,
            "hello",
            "hi",
            &SearchOptions::default(),
            &HashSet::new(),
        )
        .unwrap();
        assert_eq!(rr.replacement_count, 0);
        // File should be unchanged
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("hello world"));
    }
}
