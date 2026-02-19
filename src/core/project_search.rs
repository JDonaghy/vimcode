//! Project-wide file search (no regex, case-insensitive literal match).
//!
//! Entirely in `core/` — no UI dependencies.

use std::fs;
use std::path::{Path, PathBuf};

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

/// Search all text files under `root` for `query` (case-insensitive literal).
///
/// - Hidden files/directories (names starting with `.`) are skipped.
/// - Files that cannot be decoded as UTF-8 are silently skipped (binary).
/// - Results are sorted by file path, then by line number.
/// - Returns an empty `Vec` if `query` is empty.
pub fn search_in_project(root: &Path, query: &str) -> Vec<ProjectMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let lower_query = query.to_lowercase();
    let mut results: Vec<ProjectMatch> = Vec::new();
    walk_dir(root, &lower_query, &mut results);
    results.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    results
}

fn walk_dir(dir: &Path, lower_query: &str, out: &mut Vec<ProjectMatch>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| !n.starts_with('.'))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();

    for path in paths {
        if path.is_dir() {
            walk_dir(&path, lower_query, out);
        } else {
            search_file(&path, lower_query, out);
        }
    }
}

fn search_file(path: &Path, lower_query: &str, out: &mut Vec<ProjectMatch>) {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return, // binary or unreadable — skip silently
    };
    for (line_idx, line_text) in content.lines().enumerate() {
        let lower_line = line_text.to_lowercase();
        if let Some(col) = lower_line.find(lower_query) {
            out.push(ProjectMatch {
                file: path.to_path_buf(),
                line: line_idx,
                col,
                line_text: line_text.to_string(),
            });
        }
    }
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
        let results = search_in_project(&dir, "");
        assert!(results.is_empty());
    }

    #[test]
    fn test_case_insensitive_match() {
        let dir = make_temp_project("case_insensitive");
        let results = search_in_project(&dir, "hello");
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
        let results = search_in_project(&dir, "zzznomatch");
        assert!(results.is_empty());
    }

    #[test]
    fn test_hidden_dirs_skipped() {
        let dir = make_temp_project("hidden_dirs");
        let results = search_in_project(&dir, "hidden");
        assert!(
            results.is_empty(),
            "hidden directory should be skipped, got: {:?}",
            results
        );
    }

    #[test]
    fn test_col_offset() {
        let dir = make_temp_project("col_offset");
        let results = search_in_project(&dir, "world");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].col, 6); // "hello " = 6 bytes
    }
}
