use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use super::buffer::{Buffer, BufferId};
use super::cursor::Cursor;
use super::syntax::Syntax;

/// Upper bound on line count for tree-sitter highlighting.
///
/// Buffers with more lines than this skip the expensive `Syntax::parse()` call
/// in [`BufferState::update_syntax`] and render as plain text. Seeded by
/// [`Engine::new`](super::engine::Engine::new) from `Settings::syntax_max_lines`
/// and resynced on `:set syntax_max_lines=…`.
static SYNTAX_MAX_LINES: AtomicUsize = AtomicUsize::new(20_000);

/// Update the process-wide syntax-highlighting line-count threshold.
/// Thread-safe; cheap to call on every `:set` change.
pub fn set_syntax_max_lines(n: usize) {
    SYNTAX_MAX_LINES.store(n, Ordering::Relaxed);
}

/// Current process-wide syntax-highlighting line-count threshold.
pub fn syntax_max_lines() -> usize {
    SYNTAX_MAX_LINES.load(Ordering::Relaxed)
}

/// Line ending format for a buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    LF,
    Crlf,
}

impl LineEnding {
    /// Detect line ending from file content bytes. Scans up to 8KB.
    pub fn detect(text: &str) -> Self {
        let mut end = text.len().min(8192);
        // Back up to a valid char boundary (multi-byte chars may straddle 8KB)
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        let scan = &text[..end];
        if scan.contains("\r\n") {
            LineEnding::Crlf
        } else {
            LineEnding::LF
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::LF => "LF",
            LineEnding::Crlf => "CRLF",
        }
    }
}

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
    /// Canonicalized (symlink-resolved, absolute) version of `file_path`.
    /// Computed once on file open and cached so renderers don't need to call
    /// `canonicalize()` (a filesystem syscall) on every frame.
    pub canonical_path: Option<PathBuf>,
    /// Whether the buffer has unsaved changes.
    pub dirty: bool,
    /// Undo stack depth at the time of last save (used to detect clean state after undo/redo).
    /// `None` means never saved (new buffer).
    pub saved_undo_depth: Option<usize>,
    /// Whether this is a preview buffer (single-click in file explorer).
    pub preview: bool,
    /// For diff buffers: the source file the diff was generated from.
    pub source_file: Option<PathBuf>,
    /// Syntax highlighter for this buffer (`None` for plain text / unrecognised extensions).
    pub syntax: Option<Syntax>,
    /// Cached syntax highlights (byte ranges + scope names).
    pub highlights: Vec<(usize, usize, String)>,
    /// Whether highlights are stale (tree was re-parsed but highlights not yet re-extracted).
    pub syntax_stale: bool,
    /// When the syntax was last marked stale (for debounced re-parse in insert mode).
    pub syntax_stale_since: Option<std::time::Instant>,
    /// Undo stack (most recent at the end).
    pub undo_stack: Vec<UndoEntry>,
    /// Redo stack (most recent at the end).
    pub redo_stack: Vec<UndoEntry>,
    /// Current undo group being accumulated (during Insert mode or multi-op commands).
    pub current_undo_group: Option<UndoEntry>,
    /// Original line content for U (undo line) command: (line_number, original_content)
    pub line_undo_state: Option<(usize, String)>,
    /// Chronological timeline of buffer states for `g-`/`g+` navigation.
    /// Each entry is (buffer_text, cursor). Capped at `UNDO_TIMELINE_MAX`.
    pub undo_timeline: Vec<(String, Cursor)>,
    /// Current position in the undo timeline (index into `undo_timeline`).
    /// `None` means we're at the latest state (no g-/g+ active).
    pub undo_timeline_pos: Option<usize>,
    /// Per-line git diff status (Added/Modified/Deleted/None). Empty when not in a git repo.
    pub git_diff: Vec<Option<crate::core::git::GitLineStatus>>,
    /// Structured diff hunks for the working copy, cached from `compute_file_diff_hunks`.
    pub diff_hunks: Vec<crate::core::git::DiffHunkInfo>,
    /// LSP language identifier (e.g. "rust", "python") for this buffer, if applicable.
    pub lsp_language_id: Option<String>,
    /// Cached maximum line length (in chars) across the whole buffer.
    /// Recomputed in `update_syntax` so renders don't need to scan every line.
    pub max_col: usize,
    /// Whether this buffer is read-only (e.g. markdown preview).
    pub read_only: bool,
    /// Pre-rendered markdown content (set for markdown preview buffers).
    pub md_rendered: Option<crate::core::markdown::MdRendered>,
    /// LSP semantic tokens (decoded, absolute positions). Overlays tree-sitter highlights.
    pub semantic_tokens: Vec<crate::core::lsp::SemanticToken>,
    /// For netrw buffers: the directory currently being listed.
    pub netrw_dir: Option<PathBuf>,
    /// Whether this buffer is a keymaps editor scratch buffer.
    pub is_keymaps_buf: bool,
    /// Whether this buffer is an extension registries editor scratch buffer.
    pub is_registries_buf: bool,
    /// Whether this buffer is a command-line window (`q:` / `q/` / `q?`).
    pub is_cmdline_buf: bool,
    /// If true, the command-line window is for search history; if false, for command history.
    pub cmdline_is_search: bool,
    /// Display name for plugin-created scratch buffers (shown in tab bar).
    pub scratch_name: Option<String>,
    /// Override display name without brackets (e.g. for diff tabs).
    pub diff_label: Option<String>,
    /// Last-known modification time of the file on disk.
    /// Set on file open and save; used by `check_file_changes()` to detect external edits.
    pub file_mtime: Option<SystemTime>,
    /// Whether a "file changed on disk" warning has already been shown for the
    /// current external modification.  Reset when the mtime is updated (reload / save).
    pub file_change_warned: bool,
    /// Auto-detected indent width from the file's existing content.
    /// When `Some(n)`, overrides `settings.shift_width` for this buffer.
    /// Detected on file open by analyzing indent deltas between lines.
    pub detected_indent: Option<u8>,
    /// Line ending format (LF or CRLF). Detected on file open, default LF.
    pub line_ending: LineEnding,
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
            canonical_path: None,
            dirty: false,
            saved_undo_depth: None,
            preview: false,
            source_file: None,
            syntax: None,
            highlights: Vec::new(),
            syntax_stale: false,
            syntax_stale_since: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_undo_group: None,
            line_undo_state: None,
            undo_timeline: Vec::new(),
            undo_timeline_pos: None,
            git_diff: Vec::new(),
            diff_hunks: Vec::new(),
            lsp_language_id: None,
            max_col: 0,
            read_only: false,
            md_rendered: None,
            semantic_tokens: Vec::new(),
            netrw_dir: None,
            is_keymaps_buf: false,
            is_registries_buf: false,
            is_cmdline_buf: false,
            cmdline_is_search: false,
            scratch_name: None,
            diff_label: None,
            file_mtime: None,
            file_change_warned: false,
            detected_indent: None,
            line_ending: LineEnding::LF,
        };
        state.update_syntax();
        state
    }

    pub fn with_file(buffer: Buffer, path: PathBuf) -> Self {
        let syntax = Syntax::new_from_path(path.to_str());
        let lsp_language_id = crate::core::lsp::language_id_from_path(&path);
        let canonical_path = path.canonicalize().ok();
        let file_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let line_ending = LineEnding::detect(&buffer.to_string());

        let mut state = Self {
            buffer,
            canonical_path,
            file_path: Some(path),
            dirty: false,
            saved_undo_depth: Some(0),
            preview: false,
            source_file: None,
            syntax,
            highlights: Vec::new(),
            syntax_stale: false,
            syntax_stale_since: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            current_undo_group: None,
            line_undo_state: None,
            undo_timeline: Vec::new(),
            undo_timeline_pos: None,
            git_diff: Vec::new(),
            diff_hunks: Vec::new(),
            lsp_language_id,
            max_col: 0,
            read_only: false,
            md_rendered: None,
            semantic_tokens: Vec::new(),
            netrw_dir: None,
            is_keymaps_buf: false,
            is_registries_buf: false,
            is_cmdline_buf: false,
            cmdline_is_search: false,
            scratch_name: None,
            diff_label: None,
            file_mtime,
            file_change_warned: false,
            detected_indent: None,
            line_ending,
        };
        state.detect_indent();
        state.update_syntax();
        state
    }

    /// Re-parse the buffer and update syntax highlights and max_col cache.
    pub fn update_syntax(&mut self) {
        self.update_syntax_with_limit(syntax_max_lines());
    }

    /// Like [`update_syntax`] but with an explicit line-count threshold.
    /// Skips tree-sitter parsing when the buffer exceeds `max_lines` — the
    /// dominant startup cost for generated files (Cargo.lock, logs, etc.),
    /// which blocks the main thread for seconds. Keeps `self.syntax`
    /// installed so raising the limit and calling this again re-enables
    /// highlighting without reopening the file.
    ///
    /// Exists separately from `update_syntax` so tests can exercise the gate
    /// without racing on the process-wide [`SYNTAX_MAX_LINES`] atomic.
    pub fn update_syntax_with_limit(&mut self, max_lines: usize) {
        let text = self.buffer.to_string();
        let over_limit = self.buffer.content.len_lines() > max_lines;
        self.highlights = if over_limit {
            Vec::new()
        } else if let Some(ref mut syn) = self.syntax {
            let mut hl = syn.parse(&text);
            // Ensure sorted by start_byte — the render pipeline uses binary
            // search (partition_point) to narrow highlights to the viewport.
            hl.sort_by_key(|h| h.0);
            hl
        } else {
            Vec::new()
        };
        // Cache max line length while we have the text; avoids O(N) scan every render.
        self.max_col = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    }

    /// Analyze the buffer's existing indentation to detect the indent width.
    /// Looks at indent deltas between consecutive non-empty lines and picks
    /// the most common delta.  Sets `detected_indent` to `Some(n)` if a
    /// consistent pattern is found, or `None` if the file is empty / has no
    /// indented lines.
    pub fn detect_indent(&mut self) {
        let mut counts = [0u32; 9]; // counts[1..8] = how many deltas of that size
        let mut prev_indent: Option<usize> = None;

        for line in self.buffer.content.lines() {
            let text: String = line.chars().collect();
            let trimmed = text.trim_end_matches(['\n', '\r']);
            if trimmed.is_empty() {
                continue;
            }
            // Count leading spaces (tabs count as 1 unit for detection purposes)
            let indent: usize = trimmed
                .chars()
                .take_while(|&c| c == ' ' || c == '\t')
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum();

            if let Some(prev) = prev_indent {
                let delta = indent.abs_diff(prev);
                if delta > 0 && delta <= 8 {
                    counts[delta] += 1;
                }
            }
            prev_indent = Some(indent);
        }

        // Find the most common non-zero delta
        let best = counts[1..]
            .iter()
            .enumerate()
            .max_by_key(|&(_, &count)| count)
            .filter(|&(_, &count)| count >= 2) // need at least 2 occurrences
            .map(|(i, _)| (i + 1) as u8);

        self.detected_indent = best;
    }

    /// Mark syntax as needing a re-parse. Does NO work — just records the
    /// timestamp so the idle handler can debounce and re-parse after the user
    /// pauses typing. Call this on every keystroke in insert mode.
    #[allow(dead_code)]
    pub fn mark_syntax_stale(&mut self) {
        self.syntax_stale = true;
        self.syntax_stale_since = Some(std::time::Instant::now());
    }

    /// Full re-parse + highlight extraction if syntax is stale.
    /// Called on insert mode exit (Escape) where we need full highlights.
    pub fn refresh_syntax_if_stale(&mut self) {
        if !self.syntax_stale {
            return;
        }
        self.syntax_stale = false;
        self.syntax_stale_since = None;
        self.update_syntax();
    }

    /// Re-parse + extract highlights only for the visible viewport.
    /// Much faster than full extraction for large files.
    #[allow(dead_code)]
    pub fn refresh_syntax_visible(&mut self, scroll_top: usize, visible_lines: usize) {
        if !self.syntax_stale {
            return;
        }
        self.syntax_stale = false;
        self.syntax_stale_since = None;
        let text = self.buffer.to_string();
        if let Some(ref mut syn) = self.syntax {
            syn.reparse(&text);
            let total_lines = self.buffer.len_lines();
            let start_line = scroll_top.min(total_lines);
            let end_line = (scroll_top + visible_lines + 1).min(total_lines);
            let start_byte = self.buffer.content.line_to_byte(start_line);
            let end_byte = if end_line < total_lines {
                self.buffer.content.line_to_byte(end_line)
            } else {
                self.buffer.content.len_bytes()
            };
            self.highlights = syn.extract_highlights_range(&text, start_byte, end_byte);
        }
        self.max_col = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
    }

    /// Switch line ending format. Converts all line endings in the buffer content.
    pub fn set_line_ending(&mut self, new: LineEnding) {
        if self.line_ending == new {
            return;
        }
        let text = self.buffer.to_string();
        let converted = match new {
            LineEnding::Crlf => text.replace('\n', "\r\n"),
            LineEnding::LF => text.replace("\r\n", "\n"),
        };
        let char_len = self.buffer.len_chars();
        self.buffer.delete_range(0, char_len);
        if !converted.is_empty() {
            self.buffer.insert(0, &converted);
        }
        self.line_ending = new;
        self.dirty = true;
    }

    /// Save the buffer to its associated file path.
    pub fn save(&mut self) -> Result<usize, io::Error> {
        if let Some(ref path) = self.file_path {
            self.buffer.save_to_file(path)?;
            self.dirty = false;
            self.saved_undo_depth = Some(self.undo_stack.len());
            self.file_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            self.file_change_warned = false;
            Ok(self.buffer.len_lines())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "No file name"))
        }
    }

    /// Re-read the file from disk, replacing all buffer content.
    /// Resets dirty flag, undo/redo stacks, and updates mtime.
    pub fn reload_from_disk(&mut self) -> Result<(), io::Error> {
        if let Some(ref path) = self.file_path {
            let text = std::fs::read_to_string(path)?;
            self.line_ending = LineEnding::detect(&text);
            let char_len = self.buffer.len_chars();
            self.buffer.delete_range(0, char_len);
            if !text.is_empty() {
                self.buffer.insert(0, &text);
            }
            self.dirty = false;
            self.saved_undo_depth = Some(0);
            self.undo_stack.clear();
            self.redo_stack.clear();
            self.current_undo_group = None;
            self.undo_timeline.clear();
            self.undo_timeline_pos = None;
            self.file_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            self.file_change_warned = false;
            self.update_syntax();
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "No file name"))
        }
    }

    /// Get the display name for this buffer (filename or "[No Name]").
    pub fn display_name(&self) -> String {
        if self.is_keymaps_buf {
            return "[Keymaps]".to_string();
        }
        if self.is_registries_buf {
            return "[Registries]".to_string();
        }
        if let Some(ref label) = self.diff_label {
            return label.clone();
        }
        if let Some(ref name) = self.scratch_name {
            return format!("[{}]", name);
        }
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

    /// Maximum number of timeline snapshots to keep.
    const UNDO_TIMELINE_MAX: usize = 200;

    /// Record a timeline snapshot of the current buffer state + cursor.
    pub fn record_timeline_snapshot(&mut self, cursor: Cursor) {
        let text = self.buffer.to_string();
        // If we navigated backward via g- and then edited, trim future entries
        if let Some(pos) = self.undo_timeline_pos {
            self.undo_timeline.truncate(pos + 1);
        }
        self.undo_timeline_pos = None;
        self.undo_timeline.push((text, cursor));
        // Cap size
        if self.undo_timeline.len() > Self::UNDO_TIMELINE_MAX {
            self.undo_timeline.remove(0);
        }
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
                    let safe_pos = (*pos).min(self.buffer.len_chars().saturating_sub(1));
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

    /// Check if the buffer content matches the last-saved state, based on undo stack depth.
    pub fn is_at_saved_state(&self) -> bool {
        match self.saved_undo_depth {
            Some(depth) => self.undo_stack.len() == depth && self.current_undo_group.is_none(),
            // Never saved: consider clean only if no edits at all.
            None => self.undo_stack.is_empty() && self.current_undo_group.is_none(),
        }
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
    #[allow(dead_code)]
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

    /// Remove a buffer by ID.
    pub fn remove(&mut self, id: BufferId) {
        self.buffers.remove(&id);
    }

    /// Create a new empty buffer and return its ID.
    pub fn create(&mut self) -> BufferId {
        let id = BufferId(self.next_id);
        self.next_id += 1;
        let buffer = Buffer::new(id);
        self.buffers.insert(id, BufferState::new(buffer));
        id
    }

    /// Apply user language_map overrides to a buffer's lsp_language_id.
    pub fn apply_language_map(
        &mut self,
        id: BufferId,
        language_map: &std::collections::HashMap<String, String>,
    ) {
        if language_map.is_empty() {
            return;
        }
        if let Some(state) = self.buffers.get_mut(&id) {
            if let Some(ext) = state
                .file_path
                .as_ref()
                .and_then(|p| p.extension())
                .and_then(|e| e.to_str())
            {
                if let Some(lang) = language_map.get(ext) {
                    state.lsp_language_id = Some(lang.clone());
                }
            }
        }
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
    /// Iterate over all (BufferId, BufferState) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&BufferId, &BufferState)> {
        self.buffers.iter()
    }

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

    /// Covers the line-count gate in [`BufferState::update_syntax_with_limit`].
    ///
    /// Uses the `_with_limit` variant (not the atomic-reading `update_syntax`)
    /// so parallel tests that call `Engine::new` — which writes to the
    /// process-wide [`SYNTAX_MAX_LINES`] atomic — can't interfere.
    #[test]
    fn test_syntax_max_lines_gate() {
        use crate::core::syntax::{Syntax, SyntaxLanguage};

        // Case 1: small buffer under a generous threshold — highlights populate.
        let mut small = BufferState::new(Buffer::new(crate::core::buffer::BufferId(0)));
        small.syntax = Some(Syntax::new_for_language(SyntaxLanguage::Rust));
        small.buffer.insert(0, "fn main() { let x = 42; }");
        small.update_syntax_with_limit(20_000);
        assert!(
            !small.highlights.is_empty(),
            "small buffer under threshold should get highlights"
        );

        // Case 2: buffer over a low threshold — parse is skipped, highlights
        // empty, Syntax still installed so raising the limit re-enables it.
        let mut big = BufferState::new(Buffer::new(crate::core::buffer::BufferId(0)));
        big.syntax = Some(Syntax::new_for_language(SyntaxLanguage::Rust));
        big.buffer.insert(
            0,
            "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\nfn g() {}\nfn h() {}\nfn i() {}\nfn j() {}\n",
        );
        big.update_syntax_with_limit(5);
        assert!(
            big.highlights.is_empty(),
            "buffer over threshold should have no highlights"
        );
        assert!(big.syntax.is_some(), "syntax struct stays installed");

        // Case 3: raise threshold and re-parse — highlights populate.
        big.update_syntax_with_limit(usize::MAX);
        assert!(
            !big.highlights.is_empty(),
            "buffer should get highlights after raising the limit"
        );
    }

    // Note: the atomic-sync path (`set_syntax_max_lines` → `update_syntax`
    // reading the global) is untested because it races with any parallel
    // test that constructs an `Engine` — `Engine::new` writes the atomic
    // from `Settings::default()`. The gate logic is covered above via
    // `update_syntax_with_limit`; the production sync is a single-line
    // `set_syntax_max_lines(...)` call in `Engine::new` and `set_value_str`.
}
