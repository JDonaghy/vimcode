use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use super::buffer::{Buffer, BufferId};
use super::cursor::Cursor;
use super::syntax::Syntax;

// =============================================================================
// Undo/Redo Data Structures
// =============================================================================

/// A single text edit operation (insert or delete).
#[derive(Clone, Debug)]
pub enum EditOp {
    /// Text was inserted at position `pos`.
    Insert { pos: usize, text: String },
    /// Text was deleted from position `pos`.
    Delete { pos: usize, text: String },
}

/// A group of edits that form one undoable action.
/// In Vim, this corresponds to a single Normal mode command or an entire Insert mode session.
#[derive(Clone, Debug)]
pub struct UndoEntry {
    /// The operations in this undo group (in order of execution).
    pub ops: Vec<EditOp>,
    /// Cursor position before the operations (restored on undo).
    pub cursor_before: Cursor,
}

impl UndoEntry {
    pub fn new(cursor: Cursor) -> Self {
        Self {
            ops: Vec::new(),
            cursor_before: cursor,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

// =============================================================================
// BufferState
// =============================================================================

/// Metadata for a buffer (file path, dirty state, syntax highlights, undo history).
pub struct BufferState {
    pub buffer: Buffer,
    /// Path to the file being edited, if any.
    pub file_path: Option<PathBuf>,
    /// Whether the buffer has unsaved changes.
    pub dirty: bool,
    /// Whether this is a preview buffer (single-click in file explorer).
    pub preview: bool,
    /// For diff buffers: the source file the diff was generated from.
    pub source_file: Option<PathBuf>,
    /// Syntax highlighter for this buffer.
    pub syntax: Syntax,
    /// Cached syntax highlights (byte ranges + scope names).
    pub highlights: Vec<(usize, usize, String)>,
    /// Undo stack (most recent at the end).
    pub undo_stack: Vec<UndoEntry>,
    /// Redo stack (most recent at the end).
    pub redo_stack: Vec<UndoEntry>,
    /// Current undo group being accumulated (during Insert mode or multi-op commands).
    pub current_undo_group: Option<UndoEntry>,
    /// Original line content for U (undo line) command: (line_number, original_content)
    pub line_undo_state: Option<(usize, String)>,
    /// Per-line git diff status (Added/Modified/None). Empty when not in a git repo.
    pub git_diff: Vec<Option<crate::core::git::GitLineStatus>>,
}

impl std::fmt::Debug for BufferState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BufferState")
            .field("buffer", &self.buffer)
            .field("file_path", &self.file_path)
            .field("dirty", &self.dirty)
            .field("highlights", &self.highlights.len())
            .field("undo_stack", &self.undo_stack.len())
            .field("redo_stack", &self.redo_stack.len())
            .finish()
    }
}

impl BufferState {
    pub fn new(buffer: Buffer) -> Self {
        let mut state = Self {
            buffer,
            file_path: None,
            dirty: false,
            preview: false,
            source_file: None,
            syntax: Syntax::new(),
            highlights: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_undo_group: None,
            line_undo_state: None,
            git_diff: Vec::new(),
        };
        state.update_syntax();
        state
    }

    pub fn with_file(buffer: Buffer, path: PathBuf) -> Self {
        // Try to detect language from file path, fallback to Rust
        let syntax = Syntax::new_from_path(path.to_str()).unwrap_or_else(Syntax::new);

        let mut state = Self {
            buffer,
            file_path: Some(path),
            dirty: false,
            preview: false,
            source_file: None,
            syntax,
            highlights: Vec::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_undo_group: None,
            line_undo_state: None,
            git_diff: Vec::new(),
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

    // =========================================================================
    // Undo/Redo Methods
    // =========================================================================

    /// Start a new undo group. Call this before a series of related edits.
    /// For Insert mode, call this when entering Insert mode.
    /// For Normal mode commands, call this before executing the command.
    pub fn start_undo_group(&mut self, cursor: Cursor) {
        // If there's already a group in progress, finish it first
        self.finish_undo_group();
        self.current_undo_group = Some(UndoEntry::new(cursor));
    }

    /// Record an insert operation in the current undo group.
    pub fn record_insert(&mut self, pos: usize, text: &str) {
        if let Some(ref mut group) = self.current_undo_group {
            group.ops.push(EditOp::Insert {
                pos,
                text: text.to_string(),
            });
        }
        // Clear redo stack on any new edit
        self.redo_stack.clear();
    }

    /// Record a delete operation in the current undo group.
    /// `text` is the text that was deleted (needed for undo).
    pub fn record_delete(&mut self, pos: usize, text: &str) {
        if let Some(ref mut group) = self.current_undo_group {
            group.ops.push(EditOp::Delete {
                pos,
                text: text.to_string(),
            });
        }
        // Clear redo stack on any new edit
        self.redo_stack.clear();
    }

    /// Finish the current undo group and push it to the undo stack.
    /// Call this after a Normal mode command completes, or when leaving Insert mode.
    pub fn finish_undo_group(&mut self) {
        if let Some(group) = self.current_undo_group.take() {
            if !group.is_empty() {
                self.undo_stack.push(group);
            }
        }
    }

    /// Undo the last change. Returns the cursor position to restore, or None if nothing to undo.
    pub fn undo(&mut self) -> Option<Cursor> {
        // Finish any in-progress group first
        self.finish_undo_group();

        let entry = self.undo_stack.pop()?;
        let cursor_to_restore = entry.cursor_before;

        // Build the redo entry by recording the inverse operations
        let mut redo_ops = Vec::new();

        // Apply inverse operations in reverse order
        for op in entry.ops.iter().rev() {
            match op {
                EditOp::Insert { pos, text } => {
                    // Undo an insert by deleting the text
                    let end = pos + text.chars().count();
                    self.buffer.delete_range(*pos, end);
                    // For redo, we'll need to re-insert
                    redo_ops.push(EditOp::Insert {
                        pos: *pos,
                        text: text.clone(),
                    });
                }
                EditOp::Delete { pos, text } => {
                    // Undo a delete by re-inserting the text
                    self.buffer.insert(*pos, text);
                    // For redo, we'll need to delete again
                    redo_ops.push(EditOp::Delete {
                        pos: *pos,
                        text: text.clone(),
                    });
                }
            }
        }

        // Reverse redo_ops so they're in the correct order for redo
        redo_ops.reverse();

        // Push to redo stack with the current cursor position
        // (which will be restored if they redo)
        self.redo_stack.push(UndoEntry {
            ops: entry.ops,
            cursor_before: cursor_to_restore,
        });

        self.update_syntax();
        Some(cursor_to_restore)
    }

    /// Redo the last undone change. Returns the cursor position after redo, or None if nothing to redo.
    pub fn redo(&mut self) -> Option<Cursor> {
        let entry = self.redo_stack.pop()?;

        // Calculate cursor position after redo (end of last operation)
        let mut cursor_after = entry.cursor_before;

        // Re-apply the operations in forward order
        for op in entry.ops.iter() {
            match op {
                EditOp::Insert { pos, text } => {
                    self.buffer.insert(*pos, text);
                    // Position cursor at end of inserted text
                    let line = self
                        .buffer
                        .content
                        .char_to_line(*pos + text.chars().count());
                    let line_start = self.buffer.line_to_char(line);
                    cursor_after = Cursor {
                        line,
                        col: (*pos + text.chars().count()) - line_start,
                    };
                }
                EditOp::Delete { pos, text } => {
                    // Delete the text that was originally deleted
                    let end = pos + text.chars().count();
                    self.buffer.delete_range(*pos, end);
                    // Position cursor at the deletion point
                    let safe_pos = (*pos).min(self.buffer.len_chars().saturating_sub(1).max(0));
                    let line = if self.buffer.len_chars() == 0 {
                        0
                    } else {
                        self.buffer.content.char_to_line(safe_pos)
                    };
                    let line_start = self.buffer.line_to_char(line);
                    cursor_after = Cursor {
                        line,
                        col: pos.saturating_sub(line_start),
                    };
                }
            }
        }

        // Push back to undo stack
        self.undo_stack.push(entry);

        self.update_syntax();
        Some(cursor_after)
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
            || self
                .current_undo_group
                .as_ref()
                .is_some_and(|g| !g.is_empty())
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Save the original content of a line before modifications (for U command)
    pub fn save_line_for_undo(&mut self, line_num: usize) {
        // Only save if we haven't already saved this line
        if let Some((saved_line, _)) = self.line_undo_state {
            if saved_line == line_num {
                return; // Already saved this line
            }
        }

        // Save the current line content
        if line_num < self.buffer.len_lines() {
            let line_content: String = self.buffer.content.line(line_num).chars().collect();
            self.line_undo_state = Some((line_num, line_content));
        }
    }

    /// Undo all changes on the current line (U command)
    pub fn undo_line(&mut self, current_line: usize, cursor: Cursor) -> Option<Cursor> {
        let (saved_line, original_content) = self.line_undo_state.take()?;

        // Only undo if we're on the saved line
        if saved_line != current_line {
            return None;
        }

        // Get the current line content
        if current_line >= self.buffer.len_lines() {
            return None;
        }

        let line_start = self.buffer.line_to_char(current_line);
        let line_len = self.buffer.line_len_chars(current_line);
        let line_end = line_start + line_len;

        // Start an undo group for the line restore
        self.start_undo_group(cursor);

        // Delete the current line content and insert the original
        if line_len > 0 {
            let deleted_text: String = self
                .buffer
                .content
                .slice(line_start..line_end)
                .chars()
                .collect();
            self.record_delete(line_start, &deleted_text);
            self.buffer.delete_range(line_start, line_end);
        }
        self.record_insert(line_start, &original_content);
        self.buffer.insert(line_start, &original_content);

        self.finish_undo_group();
        self.update_syntax();

        // Return cursor at start of line
        Some(Cursor {
            line: current_line,
            col: 0,
        })
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
