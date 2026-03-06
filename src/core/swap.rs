//! Swap file I/O for crash recovery.
//!
//! Each open buffer with a file path gets a swap file under
//! `~/.config/vimcode/swap/`.  The swap file contains a short header
//! (path, PID, timestamp) followed by the raw buffer text.  On crash,
//! the stale swap file is detected on next open and recovery is offered.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Parsed swap-file header.
#[derive(Debug, Clone)]
pub struct SwapHeader {
    pub file_path: PathBuf,
    pub pid: u32,
    pub modified: String,
}

/// Directory where all swap files live.
pub fn swap_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config/vimcode/swap")
}

/// Compute the swap file path for a given canonical file path.
/// Uses FNV-1a 64-bit hash (same algorithm as `session.rs`).
pub fn swap_path_for(canonical: &Path) -> PathBuf {
    let path_str = canonical.to_string_lossy();
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in path_str.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x00000100000001b3);
    }
    swap_dir().join(format!("{:016x}.swp", hash))
}

/// Write a swap file atomically (write to `.tmp`, then rename).
pub fn write_swap(swap_path: &Path, header: &SwapHeader, content: &str) {
    if cfg!(test) || crate::core::session::saves_suppressed() {
        return;
    }
    let dir = swap_path.parent().unwrap_or(Path::new("."));
    if let Err(e) = fs::create_dir_all(dir) {
        eprintln!("swap: cannot create dir {:?}: {}", dir, e);
        return;
    }
    let tmp = swap_path.with_extension("tmp");
    let result = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        writeln!(f, "VIMCODE_SWAP_V1")?;
        writeln!(f, "path: {}", header.file_path.display())?;
        writeln!(f, "pid: {}", header.pid)?;
        writeln!(f, "modified: {}", header.modified)?;
        writeln!(f, "---")?;
        f.write_all(content.as_bytes())?;
        f.flush()?;
        fs::rename(&tmp, swap_path)?;
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("swap: write error for {:?}: {}", swap_path, e);
        let _ = fs::remove_file(&tmp);
    }
}

/// Read and parse a swap file.  Returns `None` if the file doesn't exist
/// or is malformed.
pub fn read_swap(swap_path: &Path) -> Option<(SwapHeader, String)> {
    let data = fs::read_to_string(swap_path).ok()?;
    let mut lines = data.splitn(2, "---\n");
    let header_block = lines.next()?;
    let content = lines.next().unwrap_or("");

    let mut file_path: Option<PathBuf> = None;
    let mut pid: Option<u32> = None;
    let mut modified = String::new();

    for line in header_block.lines() {
        if line == "VIMCODE_SWAP_V1" {
            continue;
        }
        if let Some(rest) = line.strip_prefix("path: ") {
            file_path = Some(PathBuf::from(rest));
        } else if let Some(rest) = line.strip_prefix("pid: ") {
            pid = rest.parse().ok();
        } else if let Some(rest) = line.strip_prefix("modified: ") {
            modified = rest.to_string();
        }
    }

    Some((
        SwapHeader {
            file_path: file_path?,
            pid: pid?,
            modified,
        },
        content.to_string(),
    ))
}

/// Delete a swap file, ignoring "not found" errors.
pub fn delete_swap(swap_path: &Path) {
    if cfg!(test) || crate::core::session::saves_suppressed() {
        return;
    }
    let _ = fs::remove_file(swap_path);
}

/// Check whether a process with the given PID is still alive.
pub fn is_pid_alive(pid: u32) -> bool {
    // On Linux, check `/proc/<pid>` existence.
    Path::new(&format!("/proc/{}", pid)).exists()
}

/// Return an ISO-8601-ish timestamp for the current moment.
pub fn now_iso8601() -> String {
    // Use UNIX_EPOCH + SystemTime for a simple UTC timestamp.
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    // Simple UTC breakdown (no chrono dependency).
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let h = time_of_day / 3600;
    let m = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;
    // Approximate date from days since epoch (good enough for display).
    let (y, mo, day) = days_to_ymd(days);
    format!("{y:04}-{mo:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Civil calendar from day count (algorithm from Howard Hinnant).
    days += 719468;
    let era = days / 146097;
    let doe = days % 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Scan the swap directory for swap files with dead PIDs.
pub fn find_stale_swaps() -> Vec<(SwapHeader, PathBuf)> {
    let dir = swap_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "swp") {
            if let Some((header, _)) = read_swap(&path) {
                if !is_pid_alive(header.pid) {
                    result.push((header, path));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_path_deterministic() {
        let p1 = swap_path_for(Path::new("/home/user/file.rs"));
        let p2 = swap_path_for(Path::new("/home/user/file.rs"));
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_swap_path_different_files() {
        let p1 = swap_path_for(Path::new("/home/user/a.rs"));
        let p2 = swap_path_for(Path::new("/home/user/b.rs"));
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_now_iso8601_format() {
        let ts = now_iso8601();
        // Should look like 2026-03-05T14:30:00Z
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20);
    }

    #[test]
    fn test_swap_roundtrip() {
        let dir = std::env::temp_dir().join("vimcode_swap_test_roundtrip");
        let _ = fs::create_dir_all(&dir);
        let swap_path = dir.join("test.swp");

        let header = SwapHeader {
            file_path: PathBuf::from("/tmp/test.rs"),
            pid: std::process::id(),
            modified: "2026-01-01T00:00:00Z".to_string(),
        };
        let content = "fn main() {\n    println!(\"hello\");\n}\n";

        // Bypass suppression for this unit test.
        let mut f = fs::File::create(&swap_path).unwrap();
        writeln!(f, "VIMCODE_SWAP_V1").unwrap();
        writeln!(f, "path: {}", header.file_path.display()).unwrap();
        writeln!(f, "pid: {}", header.pid).unwrap();
        writeln!(f, "modified: {}", header.modified).unwrap();
        writeln!(f, "---").unwrap();
        f.write_all(content.as_bytes()).unwrap();
        drop(f);

        let (parsed_header, parsed_content) = read_swap(&swap_path).unwrap();
        assert_eq!(parsed_header.file_path, header.file_path);
        assert_eq!(parsed_header.pid, header.pid);
        assert_eq!(parsed_header.modified, header.modified);
        assert_eq!(parsed_content, content);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_is_pid_alive_self() {
        assert!(is_pid_alive(std::process::id()));
    }

    #[test]
    fn test_is_pid_alive_dead() {
        // PID 999999999 is almost certainly not alive.
        assert!(!is_pid_alive(999_999_999));
    }
}
