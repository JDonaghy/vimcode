use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

    /// Last opened files (future: cursor positions)
    #[serde(default)]
    pub recent_files: Vec<PathBuf>,
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
            explorer_visible: false, // Default: hidden
            recent_files: Vec::new(),
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
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::session_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    fn session_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config/vimcode/session.json")
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
}
