use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Saved cursor and scroll position for a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePosition {
    pub line: usize,
    pub col: usize,
    pub scroll_top: usize,
}

/// Session state persisted across restarts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    /// Window geometry
    pub window: WindowGeometry,

    /// Command history (most recent last)
    #[serde(default)]
    pub command_history: Vec<String>,

    /// Search history (most recent last)
    #[serde(default)]
    pub search_history: Vec<String>,

    /// Explorer sidebar visible on startup
    #[serde(default)]
    pub explorer_visible: bool,

    /// Sidebar panel width in pixels (GTK only, default 300)
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: i32,

    /// Last opened files (future: cursor positions)
    #[serde(default)]
    pub recent_files: Vec<PathBuf>,

    /// Saved cursor/scroll positions per file (keyed by canonical path)
    #[serde(default)]
    pub file_positions: HashMap<PathBuf, FilePosition>,

    /// All files that were open when the session was last saved (for restore on startup)
    #[serde(default)]
    pub open_files: Vec<PathBuf>,

    /// The active (focused) file when the session was last saved
    #[serde(default)]
    pub active_file: Option<PathBuf>,

    /// Terminal panel content rows (default 12; does not include the header row)
    #[serde(default = "default_terminal_rows")]
    pub terminal_panel_rows: u16,

    /// Recently opened workspace root paths (last 10, stored in global session only).
    #[serde(default)]
    pub recent_workspaces: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub width: i32,
    pub height: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    #[serde(default)]
    pub maximized: bool,
}

fn default_sidebar_width() -> i32 {
    260
}

fn default_terminal_rows() -> u16 {
    12
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            window: WindowGeometry {
                width: 800,
                height: 600,
                x: None,
                y: None,
                maximized: false,
            },
            command_history: Vec::new(),
            search_history: Vec::new(),
            explorer_visible: false,
            sidebar_width: default_sidebar_width(),
            recent_files: Vec::new(),
            file_positions: HashMap::new(),
            open_files: Vec::new(),
            active_file: None,
            terminal_panel_rows: default_terminal_rows(),
            recent_workspaces: Vec::new(),
        }
    }
}

impl SessionState {
    /// Load session state from ~/.config/vimcode/session.json
    pub fn load() -> Self {
        let path = Self::session_path();
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str(&contents) {
                return state;
            }
        }
        Self::default()
    }

    /// Save session state to ~/.config/vimcode/session.json
    ///
    /// Uses an atomic write: serialise → write to `.tmp` → rename.
    /// A rename is atomic on Linux/macOS (same filesystem), so a crash
    /// mid-write cannot corrupt the existing session file.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::session_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn session_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config/vimcode/session.json")
    }

    /// Compute a stable per-workspace session path based on the workspace root.
    /// Uses a simple FNV-1a 64-bit hash of the canonical path string.
    pub fn session_path_for_workspace(root: &Path) -> PathBuf {
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let path_str = canonical.to_string_lossy();
        // FNV-1a 64-bit hash (deterministic, no external crates needed)
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in path_str.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x00000100000001b3);
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".config/vimcode/sessions")
            .join(format!("{:016x}.json", hash))
    }

    /// Load per-workspace session state (open files, positions, etc.).
    /// Falls back to an empty session if the file does not exist.
    pub fn load_for_workspace(root: &Path) -> Self {
        let path = Self::session_path_for_workspace(root);
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Ok(state) = serde_json::from_str(&contents) {
                return state;
            }
        }
        Self::default()
    }

    /// Save per-workspace session state to the per-project file.
    pub fn save_for_workspace(&self, root: &Path) -> std::io::Result<()> {
        let path = Self::session_path_for_workspace(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Add a workspace root to `recent_workspaces` (max 10, removes duplicates).
    pub fn add_recent_workspace(&mut self, root: &Path) {
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        self.recent_workspaces.retain(|p| p != &canonical);
        self.recent_workspaces.push(canonical);
        // Keep last 10
        while self.recent_workspaces.len() > 10 {
            self.recent_workspaces.remove(0);
        }
    }

    /// Add command to history (max 100 entries, removes duplicates)
    pub fn add_command(&mut self, cmd: &str) {
        if cmd.is_empty() {
            return;
        }
        // Remove duplicates (move to end if exists)
        self.command_history.retain(|c| c != cmd);
        self.command_history.push(cmd.to_string());
        // Keep last 100
        if self.command_history.len() > 100 {
            self.command_history.remove(0);
        }
    }

    /// Add search to history (max 100 entries, removes duplicates)
    pub fn add_search(&mut self, query: &str) {
        if query.is_empty() {
            return;
        }
        self.search_history.retain(|q| q != query);
        self.search_history.push(query.to_string());
        if self.search_history.len() > 100 {
            self.search_history.remove(0);
        }
    }

    /// Save cursor and scroll position for a file path
    pub fn save_file_position(&mut self, path: &Path, line: usize, col: usize, scroll_top: usize) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.file_positions.insert(
            canonical,
            FilePosition {
                line,
                col,
                scroll_top,
            },
        );
    }

    /// Get saved cursor/scroll position for a file path, if any
    pub fn get_file_position(&self, path: &Path) -> Option<&FilePosition> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.file_positions.get(&canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_load_default() {
        let session = SessionState::default();
        assert_eq!(session.window.width, 800);
        assert_eq!(session.window.height, 600);
        assert_eq!(session.command_history.len(), 0);
        assert_eq!(session.search_history.len(), 0);
        assert!(!session.explorer_visible);
    }

    #[test]
    fn test_add_command_history() {
        let mut session = SessionState::default();
        session.add_command("w");
        session.add_command("q");
        assert_eq!(session.command_history, vec!["w", "q"]);

        // Duplicate: moved to end
        session.add_command("w");
        assert_eq!(session.command_history, vec!["q", "w"]);
    }

    #[test]
    fn test_add_search_history() {
        let mut session = SessionState::default();
        session.add_search("hello");
        session.add_search("world");
        assert_eq!(session.search_history, vec!["hello", "world"]);

        // Duplicate: moved to end
        session.add_search("hello");
        assert_eq!(session.search_history, vec!["world", "hello"]);
    }

    #[test]
    fn test_history_limit() {
        let mut session = SessionState::default();
        for i in 0..150 {
            session.add_command(&format!("cmd{}", i));
        }
        assert_eq!(session.command_history.len(), 100);
        // Should have kept the last 100
        assert_eq!(session.command_history[0], "cmd50");
        assert_eq!(session.command_history[99], "cmd149");
    }

    #[test]
    fn test_empty_strings_ignored() {
        let mut session = SessionState::default();
        session.add_command("");
        session.add_search("");
        assert_eq!(session.command_history.len(), 0);
        assert_eq!(session.search_history.len(), 0);
    }

    #[test]
    fn test_save_and_get_file_position() {
        let mut session = SessionState::default();
        let path = Path::new("/tmp/test_vimcode_position.rs");

        // Nothing saved yet
        assert!(session.get_file_position(path).is_none());

        // Save a position
        session.save_file_position(path, 42, 7, 30);
        let pos = session.get_file_position(path).unwrap();
        assert_eq!(pos.line, 42);
        assert_eq!(pos.col, 7);
        assert_eq!(pos.scroll_top, 30);
    }

    #[test]
    fn test_file_position_overwrite() {
        let mut session = SessionState::default();
        let path = Path::new("/tmp/test_vimcode_position.rs");

        session.save_file_position(path, 10, 5, 0);
        session.save_file_position(path, 20, 3, 15);

        let pos = session.get_file_position(path).unwrap();
        assert_eq!(pos.line, 20);
        assert_eq!(pos.col, 3);
        assert_eq!(pos.scroll_top, 15);
    }

    #[test]
    fn test_file_positions_serialization() {
        let mut session = SessionState::default();
        let path = Path::new("/tmp/test_vimcode_serialize.py");
        session.save_file_position(path, 5, 2, 0);

        // Round-trip through JSON
        let json = serde_json::to_string(&session).unwrap();
        let restored: SessionState = serde_json::from_str(&json).unwrap();

        // Position should survive serialization (using canonical path)
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let pos = restored.file_positions.get(&canonical).unwrap();
        assert_eq!(pos.line, 5);
        assert_eq!(pos.col, 2);
    }

    #[test]
    fn test_workspace_session_path_is_stable() {
        let root = Path::new("/tmp");
        let path1 = SessionState::session_path_for_workspace(root);
        let path2 = SessionState::session_path_for_workspace(root);
        // Same input must always produce the same path
        assert_eq!(path1, path2);
        // Path must live under ~/.config/vimcode/sessions/
        let path_str = path1.to_string_lossy();
        assert!(path_str.contains("vimcode/sessions/"));
        assert!(path_str.ends_with(".json"));
    }

    #[test]
    fn test_workspace_session_path_differs_per_root() {
        let root_a = Path::new("/tmp/proj_a");
        let root_b = Path::new("/tmp/proj_b");
        let path_a = SessionState::session_path_for_workspace(root_a);
        let path_b = SessionState::session_path_for_workspace(root_b);
        // Different roots must hash to different files
        assert_ne!(path_a, path_b);
    }

    #[test]
    fn test_add_recent_workspace() {
        let mut session = SessionState::default();
        let root = Path::new("/tmp/my_project");
        session.add_recent_workspace(root);
        assert_eq!(session.recent_workspaces.len(), 1);

        // Adding same path twice should not duplicate
        session.add_recent_workspace(root);
        assert_eq!(session.recent_workspaces.len(), 1);

        // Adding more than 10 paths keeps only the last 10
        for i in 0..12 {
            session.add_recent_workspace(&PathBuf::from(format!("/tmp/proj_{}", i)));
        }
        assert_eq!(session.recent_workspaces.len(), 10);
    }
}
