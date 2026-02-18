use std::collections::HashMap;
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

// ─── Git status ───────────────────────────────────────────────────────────────

/// A single entry from `git status --porcelain`.
#[derive(Debug, Clone)]
pub struct StatusEntry {
    /// Two-character XY status code (e.g. "M ", " M", "??").
    pub xy: String,
    /// File path relative to repo root.
    pub path: String,
}

/// Run `git status --porcelain` and return structured entries.
/// Returns an empty vec when not in a git repo or on error.
pub fn status(dir: &Path) -> Vec<StatusEntry> {
    let output = match run_git(dir, &["status", "--porcelain"]) {
        Some(o) => o,
        None => return vec![],
    };
    output
        .lines()
        .filter(|l| l.len() >= 4)
        .map(|l| StatusEntry {
            xy: l[..2].to_string(),
            path: l[3..].to_string(),
        })
        .collect()
}

/// Format git status for display in a buffer (similar to `git status` output).
pub fn status_text(dir: &Path) -> Option<String> {
    let branch = current_branch(dir)
        .map(|b| format!("On branch {}", b))
        .unwrap_or_else(|| "HEAD detached".to_string());

    let entries = status(dir);
    if entries.is_empty() {
        return Some(format!(
            "{}\n\nnothing to commit, working tree clean\n",
            branch
        ));
    }

    let mut staged: Vec<&StatusEntry> = entries
        .iter()
        .filter(|e| e.xy.starts_with(['M', 'A', 'D', 'R', 'C']))
        .collect();
    let mut unstaged: Vec<&StatusEntry> = entries
        .iter()
        .filter(|e| {
            let c = e.xy.chars().nth(1).unwrap_or(' ');
            matches!(c, 'M' | 'D')
        })
        .collect();
    let untracked: Vec<&StatusEntry> = entries.iter().filter(|e| e.xy == "??").collect();

    // Deduplicate entries that appear in both staged and unstaged
    staged.dedup_by_key(|e| e.path.clone());
    unstaged.dedup_by_key(|e| e.path.clone());

    let mut out = format!("{}\n\n", branch);

    if !staged.is_empty() {
        out.push_str("Changes to be committed:\n");
        for e in &staged {
            let label = match e.xy.chars().next().unwrap_or(' ') {
                'M' => "modified",
                'A' => "new file",
                'D' => "deleted",
                'R' => "renamed",
                _ => "changed",
            };
            out.push_str(&format!("        {}: {}\n", label, e.path));
        }
        out.push('\n');
    }

    if !unstaged.is_empty() {
        out.push_str("Changes not staged for commit:\n");
        for e in &unstaged {
            let label = match e.xy.chars().nth(1).unwrap_or(' ') {
                'M' => "modified",
                'D' => "deleted",
                _ => "changed",
            };
            out.push_str(&format!("        {}: {}\n", label, e.path));
        }
        out.push('\n');
    }

    if !untracked.is_empty() {
        out.push_str("Untracked files:\n");
        for e in &untracked {
            out.push_str(&format!("        {}\n", e.path));
        }
        out.push('\n');
    }

    Some(out)
}

// ─── Staging ──────────────────────────────────────────────────────────────────

/// Run `git add <path>`. Returns Ok(()) on success, Err(message) on failure.
pub fn stage_file(path: &Path) -> Result<(), String> {
    let dir = path.parent().ok_or("no parent directory")?;
    let path_str = path.to_str().ok_or("invalid path")?;
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["add", path_str])
        .output()
        .map_err(|e| format!("git add failed: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "git add failed".to_string()
        } else {
            err
        })
    }
}

/// Run `git add -A` to stage all changes. Returns Ok(()) on success.
pub fn stage_all(dir: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["add", "-A"])
        .output()
        .map_err(|e| format!("git add -A failed: {}", e))?;
    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "git add -A failed".to_string()
        } else {
            err
        })
    }
}

// ─── Commit / push ────────────────────────────────────────────────────────────

/// Run `git commit -m <message>`. Returns Ok(summary) or Err(message).
pub fn commit(dir: &Path, message: &str) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err("commit message cannot be empty".to_string());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["commit", "-m", message])
        .output()
        .map_err(|e| format!("git commit failed: {}", e))?;
    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Return first line of git commit output (e.g. "[main abc1234] fix bug")
        Ok(out.lines().next().unwrap_or("committed").to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "git commit failed".to_string()
        } else {
            err
        })
    }
}

/// Run `git push`. Returns Ok(summary) or Err(message).
pub fn push(dir: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["push"])
        .output()
        .map_err(|e| format!("git push failed: {}", e))?;
    if output.status.success() {
        let out = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let err_out = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Ok(if out.is_empty() { err_out } else { out })
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "git push failed".to_string()
        } else {
            err
        })
    }
}

// ─── Blame ────────────────────────────────────────────────────────────────────

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Convert a Unix timestamp (seconds since epoch) to a "YYYY-MM-DD" string (UTC).
fn epoch_to_date(secs: i64) -> String {
    let mut remaining = (secs / 86400) as i32;
    let mut year = 1970i32;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let month_days: [i32; 12] = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1i32;
    for &m in &month_days {
        if remaining < m {
            break;
        }
        remaining -= m;
        month += 1;
    }
    format!("{:04}-{:02}-{:02}", year, month, remaining + 1)
}

/// Parse `git blame --porcelain` output into a human-readable per-line format.
///
/// Each output line has the form:
/// `<short-hash> (<author padded to 12> <YYYY-MM-DD>  <lineno>) <source content>`
fn parse_blame_porcelain(text: &str) -> String {
    // commit hash -> (author, date) — cached so repeated commits keep their info
    let mut commit_info: HashMap<String, (String, String)> = HashMap::new();
    let mut current_hash = String::new();
    let mut current_author = String::new();
    let mut current_date = String::new();
    let mut current_lineno: usize = 0;
    let mut out = String::new();

    for line in text.lines() {
        if let Some(content) = line.strip_prefix('\t') {
            // Source-content line — emit one formatted blame annotation.
            let (author, date) = commit_info
                .get(&current_hash)
                .map(|(a, d)| (a.as_str(), d.as_str()))
                .unwrap_or(("?", "????-??-??"));
            let short: String = current_hash.chars().take(8).collect();
            let author_col: String = author.chars().take(12).collect();
            out.push_str(&format!(
                "{} ({:<12} {}  {:>4}) {}\n",
                short, author_col, date, current_lineno, content
            ));
        } else if let Some(name) = line.strip_prefix("author ") {
            current_author = name.to_string();
        } else if let Some(ts) = line.strip_prefix("author-time ") {
            if let Ok(epoch) = ts.trim().parse::<i64>() {
                current_date = epoch_to_date(epoch);
            }
            // Register once we have both author and time (author line always precedes author-time).
            commit_info.insert(
                current_hash.clone(),
                (current_author.clone(), current_date.clone()),
            );
        } else {
            // Detect a blame header line: 40 hex chars, then a space, then numbers.
            let bytes = line.as_bytes();
            if bytes.len() >= 42
                && bytes[40] == b' '
                && bytes[..40].iter().all(|&b| b.is_ascii_hexdigit())
            {
                current_hash = line[..40].to_string();
                // The final line number is the 3rd space-separated field (index 2).
                let parts: Vec<&str> = line.splitn(5, ' ').collect();
                current_lineno = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                // Restore cached author/date for commits we've already seen.
                if let Some((a, d)) = commit_info.get(&current_hash) {
                    current_author = a.clone();
                    current_date = d.clone();
                }
            }
        }
    }

    out
}

/// Run `git blame --porcelain` on `path` and return a formatted blame buffer.
/// Returns `None` when the file is untracked, the repo has no commits, or
/// any other `git blame` failure occurs.
pub fn blame_text(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    let path_str = path.to_str()?;
    let raw = run_git(dir, &["blame", "--porcelain", path_str])?;
    if raw.trim().is_empty() {
        return None;
    }
    Some(parse_blame_porcelain(&raw))
}

// ─── Diff ─────────────────────────────────────────────────────────────────────

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

    // ── blame helpers ──────────────────────────────────────────────────────

    #[test]
    fn test_epoch_to_date_epoch_zero() {
        assert_eq!(epoch_to_date(0), "1970-01-01");
    }

    #[test]
    fn test_epoch_to_date_one_day() {
        assert_eq!(epoch_to_date(86400), "1970-01-02");
    }

    #[test]
    fn test_parse_blame_porcelain_single_line() {
        let hash = "a".repeat(40);
        let input = format!(
            "{h} 1 1 1\nauthor Alice\nauthor-mail <alice@example.com>\n\
             author-time 0\nauthor-tz +0000\ncommitter Alice\n\
             committer-mail <alice@example.com>\ncommitter-time 0\n\
             committer-tz +0000\nsummary Initial commit\nfilename src/main.rs\n\
             \tfn main() {{}}\n",
            h = hash
        );
        let result = parse_blame_porcelain(&input);
        assert!(result.contains("aaaaaaaa"), "should contain short hash");
        assert!(result.contains("Alice"), "should contain author");
        assert!(result.contains("1970-01-01"), "should contain date");
        assert!(
            result.contains("fn main()"),
            "should contain source content"
        );
        assert_eq!(
            result.lines().count(),
            1,
            "should produce exactly 1 output line"
        );
    }

    #[test]
    fn test_parse_blame_porcelain_repeated_commit() {
        let hash = "b".repeat(40);
        let input = format!(
            "{h} 1 1 2\nauthor Bob\nauthor-mail <bob@example.com>\n\
             author-time 86400\nauthor-tz +0000\ncommitter Bob\n\
             committer-mail <bob@example.com>\ncommitter-time 86400\n\
             committer-tz +0000\nsummary fix\nfilename f.rs\n\tline one\n\
             {h} 2 2\nfilename f.rs\n\tline two\n",
            h = hash
        );
        let result = parse_blame_porcelain(&input);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2, "should produce 2 output lines");
        assert!(
            lines[0].contains("line one"),
            "first line should contain content"
        );
        assert!(
            lines[1].contains("line two"),
            "second line should contain content"
        );
        assert!(lines[0].contains("Bob"), "first line should have author");
        assert!(
            lines[1].contains("Bob"),
            "second line should have author (cached)"
        );
    }

    #[test]
    fn test_parse_blame_porcelain_empty() {
        assert!(parse_blame_porcelain("").is_empty());
    }
}
