use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitLineStatus {
    Added,
    Modified,
}

fn run_git(dir: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
}

pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let dir = if start.is_file() {
        start.parent()?
    } else {
        start
    };
    run_git(dir, &["rev-parse", "--show-toplevel"]).map(|s| PathBuf::from(s.trim()))
}

pub fn current_branch(dir: &Path) -> Option<String> {
    run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD")
}

pub fn file_diff_text(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    let text = run_git(dir, &["diff", "HEAD", "--", path.to_str()?])?;
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

pub fn compute_file_diff(path: &Path) -> Vec<Option<GitLineStatus>> {
    let dir = match path.parent() {
        Some(d) => d,
        None => return vec![],
    };
    let path_str = match path.to_str() {
        Some(s) => s,
        None => return vec![],
    };
    let total = std::fs::read_to_string(path)
        .map(|c| c.lines().count().max(1))
        .unwrap_or(0);
    if total == 0 {
        return vec![];
    }

    // Untracked file: all lines Added (only if we're in a git repo at all)
    if run_git(dir, &["ls-files", "--error-unmatch", path_str]).is_none() {
        if find_repo_root(path).is_none() {
            return vec![];
        }
        return vec![Some(GitLineStatus::Added); total];
    }

    let diff = match run_git(dir, &["diff", "HEAD", "--", path_str]) {
        Some(d) if !d.trim().is_empty() => d,
        _ => return vec![],
    };

    parse_unified_diff(&diff, total)
}

fn parse_unified_diff(diff: &str, total_lines: usize) -> Vec<Option<GitLineStatus>> {
    let mut result = vec![None; total_lines];
    let mut new_line: usize = 0;
    let mut pending_del: usize = 0;

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("@@ ") {
            // Parse +new_start from "@@ -a,b +new_start,c @@"
            if let Some(plus) = rest.split('+').nth(1) {
                let s = plus.split([',', ' ']).next().unwrap_or("1");
                new_line = s.parse::<usize>().unwrap_or(1).saturating_sub(1);
            }
            pending_del = 0;
        } else if line.starts_with('-') && !line.starts_with("---") {
            pending_del += 1;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            if new_line < total_lines {
                result[new_line] = Some(if pending_del > 0 {
                    pending_del -= 1;
                    GitLineStatus::Modified
                } else {
                    GitLineStatus::Added
                });
            }
            new_line += 1;
        } else if !line.starts_with('\\') {
            pending_del = 0;
            new_line += 1;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_unified_diff_added_lines() {
        // Simulate adding 2 lines at the start of a 3-line file
        let diff = "@@ -0,0 +1,3 @@\n+line1\n+line2\n+line3\n";
        let result = parse_unified_diff(diff, 3);
        assert_eq!(result[0], Some(GitLineStatus::Added));
        assert_eq!(result[1], Some(GitLineStatus::Added));
        assert_eq!(result[2], Some(GitLineStatus::Added));
    }

    #[test]
    fn test_parse_unified_diff_modified_line() {
        // Simulate modifying line 2 (old line 2 removed, new line 2 added)
        let diff = "@@ -1,3 +1,3 @@\n line1\n-old line2\n+new line2\n line3\n";
        let result = parse_unified_diff(diff, 3);
        assert_eq!(result[0], None);
        assert_eq!(result[1], Some(GitLineStatus::Modified));
        assert_eq!(result[2], None);
    }

    #[test]
    fn test_parse_unified_diff_empty() {
        let result = parse_unified_diff("", 5);
        assert!(result.iter().all(|s| s.is_none()));
    }
}
