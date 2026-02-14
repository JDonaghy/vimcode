use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use super::buffer::{Buffer, BufferId};
use super::syntax::Syntax;

/// Metadata for a buffer (file path, dirty state, syntax highlights).
pub struct BufferState {
    pub buffer: Buffer,
    /// Path to the file being edited, if any.
    pub file_path: Option<PathBuf>,
    /// Whether the buffer has unsaved changes.
    pub dirty: bool,
    /// Syntax highlighter for this buffer.
    pub syntax: Syntax,
    /// Cached syntax highlights (byte ranges + scope names).
    pub highlights: Vec<(usize, usize, String)>,
}

impl std::fmt::Debug for BufferState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BufferState")
            .field("buffer", &self.buffer)
            .field("file_path", &self.file_path)
            .field("dirty", &self.dirty)
            .field("highlights", &self.highlights.len())
            .finish()
    }
}

impl BufferState {
    pub fn new(buffer: Buffer) -> Self {
        let mut state = Self {
            buffer,
            file_path: None,
            dirty: false,
            syntax: Syntax::new(),
            highlights: Vec::new(),
        };
        state.update_syntax();
        state
    }

    pub fn with_file(buffer: Buffer, path: PathBuf) -> Self {
        let mut state = Self {
            buffer,
            file_path: Some(path),
            dirty: false,
            syntax: Syntax::new(),
            highlights: Vec::new(),
        };
        state.update_syntax();
        state
    }

    /// Re-parse the buffer and update syntax highlights.
    pub fn update_syntax(&mut self) {
        let text = self.buffer.to_string();
        self.highlights = self.syntax.parse(&text);
    }

    /// Save the buffer to its associated file path.
    pub fn save(&mut self) -> Result<usize, io::Error> {
        if let Some(ref path) = self.file_path {
            self.buffer.save_to_file(path)?;
            self.dirty = false;
            Ok(self.buffer.len_lines())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "No file name"))
        }
    }

    /// Get the display name for this buffer (filename or "[No Name]").
    pub fn display_name(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "[No Name]".to_string())
    }
}

/// Manages all open buffers in the editor.
pub struct BufferManager {
    buffers: HashMap<BufferId, BufferState>,
    next_id: usize,
    /// The alternate buffer (for :b# command).
    pub alternate_buffer: Option<BufferId>,
    /// Recently opened file paths (for Ctrl-P / :e completion).
    pub recent_files: Vec<PathBuf>,
    /// Maximum number of recent files to track.
    recent_files_limit: usize,
}

impl BufferManager {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            next_id: 1,
            alternate_buffer: None,
            recent_files: Vec::new(),
            recent_files_limit: 100,
        }
    }

    /// Create a new empty buffer and return its ID.
    pub fn create(&mut self) -> BufferId {
        let id = BufferId(self.next_id);
        self.next_id += 1;
        let buffer = Buffer::new(id);
        self.buffers.insert(id, BufferState::new(buffer));
        id
    }

    /// Create a buffer from a file. Reuses existing buffer if file is already open.
    pub fn open_file(&mut self, path: &Path) -> Result<BufferId, io::Error> {
        // Check if file is already open
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        for (id, state) in &self.buffers {
            if let Some(ref existing_path) = state.file_path {
                let existing_canonical = existing_path
                    .canonicalize()
                    .unwrap_or_else(|_| existing_path.clone());
                if existing_canonical == canonical {
                    return Ok(*id);
                }
            }
        }

        // Create new buffer
        let id = BufferId(self.next_id);
        self.next_id += 1;

        let buffer_state = if path.exists() {
            let buffer = Buffer::from_file(id, path)?;
            BufferState::with_file(buffer, path.to_path_buf())
        } else {
            // New file (doesn't exist yet)
            let buffer = Buffer::new(id);
            BufferState::with_file(buffer, path.to_path_buf())
        };

        self.buffers.insert(id, buffer_state);
        self.add_recent_file(path);
        Ok(id)
    }

    /// Get a reference to a buffer state.
    pub fn get(&self, id: BufferId) -> Option<&BufferState> {
        self.buffers.get(&id)
    }

    /// Get a mutable reference to a buffer state.
    pub fn get_mut(&mut self, id: BufferId) -> Option<&mut BufferState> {
        self.buffers.get_mut(&id)
    }

    /// Delete a buffer. Returns error if buffer is dirty (unless force is true).
    pub fn delete(&mut self, id: BufferId, force: bool) -> Result<(), String> {
        if let Some(state) = self.buffers.get(&id) {
            if state.dirty && !force {
                return Err("No write since last change (add ! to override)".to_string());
            }
        }
        self.buffers.remove(&id);
        if self.alternate_buffer == Some(id) {
            self.alternate_buffer = None;
        }
        Ok(())
    }

    /// Find a buffer by partial path match.
    pub fn find_by_path(&self, query: &str) -> Option<BufferId> {
        for (id, state) in &self.buffers {
            if let Some(ref path) = state.file_path {
                let path_str = path.to_string_lossy();
                if path_str.contains(query) || path_str.ends_with(query) {
                    return Some(*id);
                }
            }
        }
        None
    }

    /// Get a list of all buffer IDs in creation order.
    pub fn list(&self) -> Vec<BufferId> {
        let mut ids: Vec<BufferId> = self.buffers.keys().copied().collect();
        ids.sort_by_key(|id| id.0);
        ids
    }

    /// Get the next buffer after the given one (for :bn).
    pub fn next_buffer(&self, current: BufferId) -> Option<BufferId> {
        let ids = self.list();
        if ids.is_empty() {
            return None;
        }
        let current_idx = ids.iter().position(|&id| id == current)?;
        let next_idx = (current_idx + 1) % ids.len();
        Some(ids[next_idx])
    }

    /// Get the previous buffer before the given one (for :bp).
    pub fn prev_buffer(&self, current: BufferId) -> Option<BufferId> {
        let ids = self.list();
        if ids.is_empty() {
            return None;
        }
        let current_idx = ids.iter().position(|&id| id == current)?;
        let prev_idx = if current_idx == 0 {
            ids.len() - 1
        } else {
            current_idx - 1
        };
        Some(ids[prev_idx])
    }

    /// Get buffer by number (1-indexed for user display).
    pub fn get_by_number(&self, num: usize) -> Option<BufferId> {
        if num == 0 {
            return None;
        }
        self.list().get(num - 1).copied()
    }

    /// Check if any buffer has unsaved changes.
    #[allow(dead_code)]
    pub fn has_dirty_buffers(&self) -> bool {
        self.buffers.values().any(|state| state.dirty)
    }

    /// Get list of dirty buffer IDs.
    #[allow(dead_code)]
    pub fn dirty_buffers(&self) -> Vec<BufferId> {
        self.buffers
            .iter()
            .filter(|(_, state)| state.dirty)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Add a file path to recent files list.
    fn add_recent_file(&mut self, path: &Path) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Remove if already present (to move to front)
        self.recent_files
            .retain(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) != canonical);

        // Add to front
        self.recent_files.insert(0, canonical);

        // Trim to limit
        if self.recent_files.len() > self.recent_files_limit {
            self.recent_files.truncate(self.recent_files_limit);
        }
    }

    /// Get number of open buffers.
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Check if there are no open buffers.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

impl Default for BufferManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_manager_create() {
        let mut manager = BufferManager::new();
        let id1 = manager.create();
        let id2 = manager.create();

        assert_ne!(id1, id2);
        assert_eq!(manager.len(), 2);
    }

    #[test]
    fn test_buffer_manager_list() {
        let mut manager = BufferManager::new();
        let id1 = manager.create();
        let id2 = manager.create();
        let id3 = manager.create();

        let list = manager.list();
        assert_eq!(list, vec![id1, id2, id3]);
    }

    #[test]
    fn test_buffer_manager_next_prev() {
        let mut manager = BufferManager::new();
        let id1 = manager.create();
        let id2 = manager.create();
        let id3 = manager.create();

        assert_eq!(manager.next_buffer(id1), Some(id2));
        assert_eq!(manager.next_buffer(id2), Some(id3));
        assert_eq!(manager.next_buffer(id3), Some(id1)); // wraps

        assert_eq!(manager.prev_buffer(id1), Some(id3)); // wraps
        assert_eq!(manager.prev_buffer(id2), Some(id1));
        assert_eq!(manager.prev_buffer(id3), Some(id2));
    }

    #[test]
    fn test_buffer_manager_delete() {
        let mut manager = BufferManager::new();
        let id1 = manager.create();
        let id2 = manager.create();

        assert!(manager.delete(id1, false).is_ok());
        assert_eq!(manager.len(), 1);
        assert!(manager.get(id1).is_none());
        assert!(manager.get(id2).is_some());
    }

    #[test]
    fn test_buffer_manager_delete_dirty_blocked() {
        let mut manager = BufferManager::new();
        let id = manager.create();
        manager.get_mut(id).unwrap().dirty = true;

        assert!(manager.delete(id, false).is_err());
        assert!(manager.delete(id, true).is_ok()); // force
    }

    #[test]
    fn test_buffer_manager_get_by_number() {
        let mut manager = BufferManager::new();
        let id1 = manager.create();
        let id2 = manager.create();

        assert_eq!(manager.get_by_number(1), Some(id1));
        assert_eq!(manager.get_by_number(2), Some(id2));
        assert_eq!(manager.get_by_number(3), None);
        assert_eq!(manager.get_by_number(0), None);
    }

    #[test]
    fn test_recent_files() {
        let mut manager = BufferManager::new();
        manager.add_recent_file(Path::new("/tmp/file1.rs"));
        manager.add_recent_file(Path::new("/tmp/file2.rs"));
        manager.add_recent_file(Path::new("/tmp/file1.rs")); // duplicate

        // file1 should be at front now
        assert_eq!(manager.recent_files.len(), 2);
    }
}
