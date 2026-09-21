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
// Undo/Redo Data Structures — Undo Tree (#1156)
// =============================================================================
//
// Undo used to be three parallel linear structures (`undo_stack`,
// `redo_stack`, `undo_timeline`) that each threw away data the others still
// needed: `redo_stack` was cleared on every edit, so an edit made after `u`
// permanently discarded the branch it moved off, and `undo_timeline` (which
// backed `g-`/`g+`) was `truncate`d at the same moment, so those couldn't
// reach the discarded branch either. This module replaces all three with a
// single tree: every edit becomes a *child* of the node you were on, so an
// edit after `u` starts a new branch instead of destroying the old one — the
// old branch stays reachable via `g-`/`g+`/`:earlier`/`:later` for as long as
// `undolevels` keeps it around.
//
// Every node stores the *full* buffer text (like the old `undo_timeline`
// did), not a diff/op-list (like the old `undo_stack` did): reconstructing
// an arbitrary node's content — needed for `g-`/`g+` to jump straight across
// branches — is then just "copy the text", with no op-replay math that has
// to be re-derived per branch. `undolevels` bounds the memory cost by
// pruning the globally-oldest branch tip that isn't on the active path.

mod undo_time {
    //! `serde` can't derive `SystemTime` directly; store it as milliseconds
    //! since the Unix epoch so undofiles are portable and human-diffable.
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(t: &SystemTime, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let millis = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        s.serialize_u64(millis)
    }

    pub fn deserialize<'de, D>(d: D) -> Result<SystemTime, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::Deserialize;
        let millis = u64::deserialize(d)?;
        Ok(UNIX_EPOCH + Duration::from_millis(millis))
    }
}

/// On-disk format version for `undofile` persistence (`:h undo-persistence`
/// equivalent). Bumped whenever [`UndoNode`]/[`UndoTree`]'s shape changes so
/// an undofile written by an older build is detected and ignored rather than
/// misparsed.
pub const UNDOFILE_VERSION: u32 = 1;

/// One recorded state in a buffer's undo tree.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UndoNode {
    /// Monotonically increasing, unique-within-this-tree sequence number.
    /// `g-`/`g+`/`:earlier`/`:later` walk nodes in `seq` order — creation
    /// order across every branch, i.e. Vim's "chronological" order — which
    /// is what lets them cross into a branch a plain `u`/edit abandoned.
    pub seq: usize,
    /// Wall-clock time the node was created. Used by `:earlier {N}m` and
    /// friends, and shown by `:undolist`.
    #[serde(with = "undo_time")]
    pub timestamp: SystemTime,
    /// Arena index of the parent node. `None` only for the root (the state
    /// before the first edit).
    pub parent: Option<usize>,
    /// Arena indices of every child ever created from this node — including
    /// ones a later `u` + new edit abandoned. They stay listed here (unless
    /// pruned by `undolevels` or folded away by `:undojoin`) so the branch
    /// remains reachable.
    pub children: Vec<usize>,
    /// The child last navigated to going *forward* from this node
    /// (`<C-r>`/`g+`/`:later` prefer this one over a sibling that was
    /// created but never actually redone into). `None` until something
    /// crosses this node moving forward.
    pub last_child: Option<usize>,
    /// Full buffer text once this node's edit is applied.
    pub text: String,
    /// Cursor position *before* the edit that produced this node — restored
    /// by `u` when leaving this node for its parent.
    pub cursor_before: Cursor,
    /// Cursor position right *after* the edit that produced this node —
    /// restored by `<C-r>`/`g+`/`:later` when arriving at this node.
    pub cursor_after: Cursor,
    /// Tombstoned by `:undojoin` (or the internal "these sub-command undo
    /// steps are one Vim-visible step" merge that `:g`, `:normal {range}`
    /// and `:folddo*` all need): folded into a later, merged node and no
    /// longer part of any live branch. Kept in the arena (so every other
    /// node's `usize` index stays valid) rather than physically removed.
    /// `g-`/`g+`/`:undolist`/redo all skip nodes with `removed: true`.
    pub removed: bool,
}

/// A buffer's full undo history: a tree of [`UndoNode`]s plus the arena
/// index of whichever one the buffer currently reflects.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct UndoTree {
    pub nodes: Vec<UndoNode>,
    /// Arena index of the node the buffer currently reflects.
    pub current: usize,
    /// Next sequence number to hand out — also, incidentally, the total
    /// number of commits this tree has ever seen (across every branch).
    pub next_seq: usize,
}

impl UndoTree {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            current: 0,
            next_seq: 0,
        }
    }

    /// Create the root node (the pre-edit state) the first time an edit
    /// happens. A no-op once the root already exists.
    fn ensure_root(&mut self, text: &str, cursor: Cursor) {
        if !self.nodes.is_empty() {
            return;
        }
        self.nodes.push(UndoNode {
            seq: 0,
            timestamp: SystemTime::now(),
            parent: None,
            children: Vec::new(),
            last_child: None,
            text: text.to_string(),
            cursor_before: cursor,
            cursor_after: cursor,
            removed: false,
        });
        self.next_seq = 1;
        self.current = 0;
    }

    /// `seq` of the node the buffer currently reflects. `0` both when the
    /// tree is empty (no edits yet) and when parked at the root — both mean
    /// "no edits applied", matching the old `undo_stack.len() == 0` check.
    pub fn current_seq(&self) -> usize {
        self.nodes.get(self.current).map(|n| n.seq).unwrap_or(0)
    }

    /// `seq` of the current node's parent — i.e. the undo step "one `u`
    /// away". `None` at the root (nothing to join with). Used by
    /// `:undojoin`, which folds the *next* committed change back into
    /// whatever step precedes the current one.
    pub fn parent_seq(&self) -> Option<usize> {
        let node = self.nodes.get(self.current)?;
        let parent = node.parent?;
        Some(self.nodes[parent].seq)
    }

    /// Commit a finished undo group as a new child of the current node, and
    /// move onto it. `cursor_before` is where the edit started (restored by
    /// a later `u`); `text_after`/`cursor_after` are the buffer/cursor once
    /// the group's edits are done.
    pub fn commit(&mut self, cursor_before: Cursor, text_after: String, cursor_after: Cursor) {
        let parent = self.current;
        let seq = self.next_seq;
        self.next_seq += 1;
        let idx = self.nodes.len();
        self.nodes.push(UndoNode {
            seq,
            timestamp: SystemTime::now(),
            parent: Some(parent),
            children: Vec::new(),
            last_child: None,
            text: text_after,
            cursor_before,
            cursor_after,
            removed: false,
        });
        self.nodes[parent].children.push(idx);
        self.nodes[parent].last_child = Some(idx);
        self.current = idx;
        self.enforce_undolevels(undo_levels());
    }

    /// `u`: move to the parent of the current node. `None` at the root
    /// (nothing to undo).
    pub fn undo(&mut self) -> Option<(String, Cursor)> {
        let node = self.nodes.get(self.current)?;
        let parent = node.parent?;
        let cursor = node.cursor_before;
        self.current = parent;
        Some((self.nodes[parent].text.clone(), cursor))
    }

    /// `<C-r>`: move to the "preferred" child of the current node — the one
    /// last redone into, if it's still live, else the most recently created
    /// live child. `None` if every child has been pruned or there are none.
    pub fn redo(&mut self) -> Option<(String, Cursor)> {
        let node = self.nodes.get(self.current)?;
        let target = node
            .last_child
            .filter(|&c| !self.nodes[c].removed)
            .or_else(|| {
                node.children
                    .iter()
                    .rev()
                    .copied()
                    .find(|&c| !self.nodes[c].removed)
            })?;
        self.nodes[self.current].last_child = Some(target);
        self.current = target;
        let n = &self.nodes[target];
        Some((n.text.clone(), n.cursor_after))
    }

    /// Whether `redo` would succeed.
    pub fn can_redo(&self) -> bool {
        self.nodes
            .get(self.current)
            .is_some_and(|n| n.children.iter().any(|&c| !self.nodes[c].removed))
    }

    /// Live node indices sorted by `seq` — the "one chronological list
    /// across every branch" `g-`/`g+`/`:earlier`/`:later`/`:undolist` all
    /// walk.
    fn live_indices_sorted(&self) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.nodes.len())
            .filter(|&i| !self.nodes[i].removed)
            .collect();
        v.sort_by_key(|&i| self.nodes[i].seq);
        v
    }

    /// `g-`/`:earlier`: move to the live node with the next-lower `seq`,
    /// globally — crossing branches, unlike a plain `u`. `None` if already
    /// at the oldest live state.
    pub fn older(&mut self) -> Option<(String, Cursor)> {
        let cur_seq = self.current_seq();
        let idx = self
            .live_indices_sorted()
            .into_iter()
            .rfind(|&i| self.nodes[i].seq < cur_seq)?;
        self.current = idx;
        let n = &self.nodes[idx];
        Some((n.text.clone(), n.cursor_after))
    }

    /// `g+`/`:later`: move to the live node with the next-higher `seq`,
    /// globally.
    pub fn newer(&mut self) -> Option<(String, Cursor)> {
        let cur_seq = self.current_seq();
        let idx = self
            .live_indices_sorted()
            .into_iter()
            .find(|&i| self.nodes[i].seq > cur_seq)?;
        self.current = idx;
        let n = &self.nodes[idx];
        Some((n.text.clone(), n.cursor_after))
    }

    /// Move to the live node with the largest `seq` whose `timestamp <=
    /// cutoff` (`:earlier {N}[smhd]`). Falls back to the oldest live node if
    /// every one postdates `cutoff`.
    pub fn at_or_before(&mut self, cutoff: SystemTime) -> Option<(String, Cursor)> {
        let live = self.live_indices_sorted();
        let idx = live
            .iter()
            .rev()
            .copied()
            .find(|&i| self.nodes[i].timestamp <= cutoff)
            .or_else(|| live.first().copied())?;
        self.current = idx;
        let n = &self.nodes[idx];
        Some((n.text.clone(), n.cursor_after))
    }

    /// Move to the live node with the smallest `seq` whose `timestamp >=
    /// cutoff` (`:later {N}[smhd]`).
    pub fn at_or_after(&mut self, cutoff: SystemTime) -> Option<(String, Cursor)> {
        let live = self.live_indices_sorted();
        let idx = live
            .iter()
            .copied()
            .find(|&i| self.nodes[i].timestamp >= cutoff)
            .or_else(|| live.last().copied())?;
        self.current = idx;
        let n = &self.nodes[idx];
        Some((n.text.clone(), n.cursor_after))
    }

    /// 1-based position of the current node within the live, `seq`-ordered
    /// list, and the total live count — the `#N/M` pair `g-`/`g+` report.
    pub fn position(&self) -> (usize, usize) {
        let live = self.live_indices_sorted();
        let total = live.len();
        let pos = live
            .iter()
            .position(|&i| i == self.current)
            .map(|p| p + 1)
            .unwrap_or(total);
        (pos, total)
    }

    /// `:undojoin` / the internal "these sub-command undo steps are one
    /// Vim-visible step" merge (`execute_global_command`, `execute_norm_range`,
    /// `execute_folddo_command` — each used to hand-roll this against the
    /// old linear `undo_stack` by draining and re-pushing `UndoEntry`s;
    /// #1156). Folds every node on the direct path from the node with `seq
    /// == mark_seq` (exclusive) to the current node (inclusive) into one
    /// node, reparented directly onto `mark_seq`'s node. A no-op if
    /// `mark_seq`'s node isn't a live ancestor of the current node (e.g. it
    /// was itself already folded away, or nothing happened since the mark).
    pub fn merge_since(&mut self, mark_seq: usize) {
        if self.current_seq() <= mark_seq {
            return;
        }
        let Some(mark_idx) = self
            .nodes
            .iter()
            .position(|n| !n.removed && n.seq == mark_seq)
        else {
            return;
        };
        // Walk from `current` back to (but not including) `mark_idx`,
        // collecting the path oldest-first.
        let mut path = Vec::new();
        let mut cur = self.current;
        while cur != mark_idx {
            path.push(cur);
            match self.nodes[cur].parent {
                Some(p) => cur = p,
                None => return, // mark_idx isn't an ancestor — leave untouched
            }
        }
        if path.is_empty() {
            return;
        }
        path.reverse();
        let first = path[0];
        let last = *path.last().unwrap();
        let cursor_before = self.nodes[first].cursor_before;
        let cursor_after = self.nodes[last].cursor_after;
        let text = self.nodes[last].text.clone();
        let children = std::mem::take(&mut self.nodes[last].children);
        let last_child = self.nodes[last].last_child;
        let seq = self.nodes[last].seq;
        let timestamp = self.nodes[first].timestamp;
        for &i in &path {
            self.nodes[i].removed = true;
        }
        let idx = self.nodes.len();
        self.nodes.push(UndoNode {
            seq,
            timestamp,
            parent: Some(mark_idx),
            children: children.clone(),
            last_child,
            text,
            cursor_before,
            cursor_after,
            removed: false,
        });
        for c in &children {
            self.nodes[*c].parent = Some(idx);
        }
        self.nodes[mark_idx].children.retain(|&c| c != first);
        self.nodes[mark_idx].children.push(idx);
        self.nodes[mark_idx].last_child = Some(idx);
        self.current = idx;
    }

    /// Enforce `'undolevels'`: while more than `limit` live nodes exist,
    /// repeatedly tombstone the globally-oldest *leaf* that isn't on the
    /// path from the root to `current` (the active branch is never pruned).
    /// Once a branch's leaf is pruned its parent may become a leaf on the
    /// next pass, so an entire abandoned branch shrinks away oldest-first.
    fn enforce_undolevels(&mut self, limit: usize) {
        let limit = limit.max(1);
        loop {
            let live_count = self.nodes.iter().filter(|n| !n.removed).count();
            if live_count <= limit {
                break;
            }
            let ancestors = self.ancestors_of(self.current);
            let victim = self
                .nodes
                .iter()
                .enumerate()
                .filter(|(i, n)| {
                    !n.removed
                        && !ancestors.contains(i)
                        && n.children.iter().all(|&c| self.nodes[c].removed)
                })
                .min_by_key(|(_, n)| n.seq)
                .map(|(i, _)| i);
            match victim {
                Some(i) => {
                    self.nodes[i].removed = true;
                    if let Some(p) = self.nodes[i].parent {
                        self.nodes[p].children.retain(|&c| c != i);
                        if self.nodes[p].last_child == Some(i) {
                            self.nodes[p].last_child = None;
                        }
                    }
                }
                // Nothing left that's safe to prune (only the active branch
                // remains) — stop even if still over `limit`.
                None => break,
            }
        }
    }

    fn ancestors_of(&self, mut idx: usize) -> std::collections::HashSet<usize> {
        let mut set = std::collections::HashSet::new();
        loop {
            set.insert(idx);
            match self.nodes.get(idx).and_then(|n| n.parent) {
                Some(p) => idx = p,
                None => break,
            }
        }
        set
    }

    /// Every live node, oldest first — the rows `:undolist` prints.
    pub fn live_nodes_for_listing(&self) -> Vec<&UndoNode> {
        self.live_indices_sorted()
            .into_iter()
            .map(|i| &self.nodes[i])
            .collect()
    }
}

/// Process-wide `'undolevels'`: the maximum number of live undo states kept
/// per buffer, across every branch. Mirrors [`SYNTAX_MAX_LINES`]'s
/// atomic-static pattern — cheap to read on every commit, thread-safe to
/// update from `:set undolevels=N`. Default `1000` matches Vim/Neovim.
static UNDO_LEVELS: AtomicUsize = AtomicUsize::new(1000);

/// Update the process-wide `'undolevels'` cap. Thread-safe; cheap to call on
/// every `:set` change.
pub fn set_undo_levels(n: usize) {
    UNDO_LEVELS.store(n, Ordering::Relaxed);
}

/// Current process-wide `'undolevels'` cap.
pub fn undo_levels() -> usize {
    UNDO_LEVELS.load(Ordering::Relaxed)
}

/// An undo group still being accumulated (Insert mode, or a multi-op Normal
/// command). Replaces the old `Option<UndoEntry>`: since every [`UndoNode`]
/// stores the *result* buffer text rather than an op list, there's nothing
/// to accumulate but "did anything actually change" and where the edit
/// started.
#[derive(Clone, Debug)]
struct PendingUndoGroup {
    cursor_before: Cursor,
    dirty: bool,
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
    /// Undo tree `seq` at the time of last save (used to detect clean state after undo/redo).
    /// `None` means never saved (new buffer).
    pub saved_undo_seq: Option<usize>,
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
    /// This buffer's full undo history (#1156).
    pub undo_tree: UndoTree,
    /// Undo group currently being accumulated (during Insert mode or a
    /// multi-op Normal command).
    current_undo_group: Option<PendingUndoGroup>,
    /// Original line content for U (undo line) command: (line_number, original_content)
    pub line_undo_state: Option<(usize, String)>,
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
    /// True once a semanticTokens response has been received for the current
    /// buffer content (even if the response was empty). Distinguishes
    /// "haven't responded yet" from "responded with zero tokens" — without
    /// this, `lsp_status_for_buffer` would keep the indicator pinned to
    /// `Initializing` forever on files where the server returns no tokens
    /// (#230). Cleared in `lsp_flush_changes` when re-requesting after an
    /// edit so the indicator can briefly reflect the new pending request.
    pub semantic_tokens_received: bool,
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
            .field("undo_tree_nodes", &self.undo_tree.nodes.len())
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
            saved_undo_seq: None,
            preview: false,
            source_file: None,
            syntax: None,
            highlights: Vec::new(),
            syntax_stale: false,
            syntax_stale_since: None,
            undo_tree: UndoTree::new(),
            current_undo_group: None,
            line_undo_state: None,
            git_diff: Vec::new(),
            diff_hunks: Vec::new(),
            lsp_language_id: None,
            max_col: 0,
            read_only: false,
            md_rendered: None,
            semantic_tokens: Vec::new(),
            semantic_tokens_received: false,
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
            saved_undo_seq: Some(0),
            preview: false,
            source_file: None,
            syntax,
            highlights: Vec::new(),
            syntax_stale: false,
            syntax_stale_since: None,
            undo_tree: UndoTree::new(),
            current_undo_group: None,
            line_undo_state: None,
            git_diff: Vec::new(),
            diff_hunks: Vec::new(),
            lsp_language_id,
            max_col: 0,
            read_only: false,
            md_rendered: None,
            semantic_tokens: Vec::new(),
            semantic_tokens_received: false,
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
        state.load_undofile_if_enabled();
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

    /// Persist this buffer's undo tree to its undofile, if `'undofile'` is
    /// on and it has an associated path. Called after every save (a fresh
    /// undofile always matches the content it accompanies — matching Vim,
    /// which also writes the undofile on `:w`).
    fn write_undofile_if_enabled(&self) {
        if !crate::core::undofile::enabled() {
            return;
        }
        let Some(path) = self.canonical_path.as_deref().or(self.file_path.as_deref()) else {
            return;
        };
        let undo_path = crate::core::undofile::path_for(path, &crate::core::undofile::dir());
        crate::core::undofile::write(&undo_path, &self.undo_tree);
    }

    /// Load a previously-persisted undo tree for this buffer's path, if
    /// `'undofile'` is on and a matching undofile exists. Called once, right
    /// after a file is opened — the buffer's freshly-read content must
    /// exactly match the undofile's `current` node's text, or `u`/`g-` would
    /// silently swap in text unrelated to what's on screen (a stale
    /// undofile from before an external edit, say), so a mismatch discards
    /// the loaded tree instead of installing it.
    fn load_undofile_if_enabled(&mut self) {
        if !crate::core::undofile::enabled() {
            return;
        }
        let Some(path) = self.canonical_path.as_deref().or(self.file_path.as_deref()) else {
            return;
        };
        let undo_path = crate::core::undofile::path_for(path, &crate::core::undofile::dir());
        let Some(tree) = crate::core::undofile::read(&undo_path) else {
            return;
        };
        let current_text = self.buffer.to_string();
        if tree
            .nodes
            .get(tree.current)
            .is_some_and(|n| n.text == current_text)
        {
            self.saved_undo_seq = Some(tree.current_seq());
            self.undo_tree = tree;
        }
    }

    /// Save the buffer to its associated file path.
    pub fn save(&mut self) -> Result<usize, io::Error> {
        if let Some(ref path) = self.file_path {
            self.buffer.save_to_file(path)?;
            self.dirty = false;
            self.saved_undo_seq = Some(self.undo_tree.current_seq());
            self.file_mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            self.file_change_warned = false;
            self.write_undofile_if_enabled();
            Ok(self.buffer.len_lines())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "No file name"))
        }
    }

    /// Re-read the file from disk, replacing all buffer content.
    /// Resets dirty flag, undo/redo stacks, and updates mtime.
    pub fn reload_from_disk(&mut self) -> Result<(), io::Error> {
        if let Some(path) = self.file_path.clone() {
            let text = std::fs::read_to_string(&path)?;
            self.line_ending = LineEnding::detect(&text);
            let char_len = self.buffer.len_chars();
            self.buffer.delete_range(0, char_len);
            if !text.is_empty() {
                self.buffer.insert(0, &text);
            }
            self.dirty = false;
            self.saved_undo_seq = Some(0);
            self.reset_undo_history();
            self.file_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
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
        // If there's already a group in progress, finish it first — `cursor`
        // (where we are *now*, about to start a new group) doubles as its
        // "cursor after" (see `finish_undo_group`'s doc comment).
        self.finish_undo_group(cursor);
        if self.undo_tree.nodes.is_empty() {
            let text = self.buffer.to_string();
            self.undo_tree.ensure_root(&text, cursor);
        }
        self.current_undo_group = Some(PendingUndoGroup {
            cursor_before: cursor,
            dirty: false,
        });
    }

    /// Record an insert operation in the current undo group.
    pub fn record_insert(&mut self, _pos: usize, _text: &str) {
        if let Some(ref mut group) = self.current_undo_group {
            group.dirty = true;
        }
    }

    /// Record a delete operation in the current undo group.
    pub fn record_delete(&mut self, _pos: usize, _text: &str) {
        if let Some(ref mut group) = self.current_undo_group {
            group.dirty = true;
        }
    }

    /// This buffer's current position in its undo tree, as a `seq` number.
    /// **Not monotonic** — `u`/`g-` move to a *lower* `seq`, `<C-r>`/`g+` to
    /// a *higher* one that already existed. Used by `:undojoin` (a mark to
    /// fold back into) and `merge_undo_since`'s call sites, where "which
    /// node are we on" is exactly what's wanted. For "did a keystroke
    /// commit a genuinely new edit" (dot-repeat bookkeeping in
    /// `engine/keys.rs`), use [`Self::undo_commit_count`] instead — that one
    /// only ever grows, so `u`/`<C-r>`/`g-`/`g+` (navigation, no new node)
    /// can't be mistaken for an edit.
    pub fn undo_seq(&self) -> usize {
        self.undo_tree.current_seq()
    }

    /// Total number of undo-tree nodes ever committed for this buffer,
    /// across every branch — unlike [`Self::undo_seq`], this only ever
    /// grows: `u`/`<C-r>`/`g-`/`g+`/`:earlier`/`:later` all just move the
    /// buffer's position among *existing* nodes, none of them call
    /// `UndoTree::commit`. Callers that used to compare `undo_stack.len()`
    /// before/after an operation to answer "did this commit a new edit"
    /// (not just "did the visible buffer change") compare this instead —
    /// using `undo_seq` there would wrongly count `u` itself as an edit,
    /// since undo *lowers* the current `seq` (#1156 regression: `x u .` on
    /// `"abc"` produced `"abc"` instead of Neovim's `"bc"`, because `u`'s
    /// `seq` drop was misread as "a new change happened", finalizing `u`
    /// itself as the next `.` target instead of leaving `x`'s recorded
    /// target alone).
    pub fn undo_commit_count(&self) -> usize {
        self.undo_tree.next_seq
    }

    /// Finish the current undo group, committing it as a new undo-tree node
    /// if it actually changed anything. Call this after a Normal mode
    /// command completes, or when leaving Insert mode. `cursor_after` is the
    /// cursor position once the group's edits are done — restored by a later
    /// `<C-r>`/`g+`/`:later` landing on this node.
    ///
    /// Returns `true` if a node was actually committed — callers use this to
    /// skip work that's only meaningful when something actually changed
    /// (#804: this used to unconditionally snapshot the *entire buffer
    /// text* on every call, including no-op calls made by insert-mode
    /// cursor movement).
    pub fn finish_undo_group(&mut self, cursor_after: Cursor) -> bool {
        if let Some(group) = self.current_undo_group.take() {
            if group.dirty {
                let text_after = self.buffer.to_string();
                self.undo_tree
                    .commit(group.cursor_before, text_after, cursor_after);
                // Deliberately *not* `write_undofile_if_enabled()` here: every
                // `UndoNode` holds the buffer's full text, so writing on every
                // commit would serialize up to `undolevels` full-buffer copies
                // to disk on every single edit. `save()` already persists the
                // undofile, matching real Vim's write-on-`:w` cadence (see
                // `write_undofile_if_enabled`'s doc comment).
                return true;
            }
        }
        false
    }

    /// Undo the last change. Returns the cursor position to restore, or None if nothing to undo.
    pub fn undo(&mut self) -> Option<Cursor> {
        // Finish any in-progress group first (shouldn't normally happen —
        // `u` only runs from Normal mode, where no group is open — but stay
        // defensive; approximate its "cursor after" with its own start
        // position since the real one isn't available here).
        let fallback_cursor = self
            .current_undo_group
            .as_ref()
            .map(|g| g.cursor_before)
            .unwrap_or_default();
        self.finish_undo_group(fallback_cursor);

        let (text, cursor) = self.undo_tree.undo()?;
        let char_len = self.buffer.len_chars();
        self.buffer.delete_range(0, char_len);
        if !text.is_empty() {
            self.buffer.insert(0, &text);
        }
        self.update_syntax();
        Some(cursor)
    }

    /// Redo the last undone change. Returns the cursor position after redo, or None if nothing to redo.
    pub fn redo(&mut self) -> Option<Cursor> {
        let (text, cursor) = self.undo_tree.redo()?;
        let char_len = self.buffer.len_chars();
        self.buffer.delete_range(0, char_len);
        if !text.is_empty() {
            self.buffer.insert(0, &text);
        }
        self.update_syntax();
        Some(cursor)
    }

    /// Install the buffer/cursor state from an undo-tree navigation result
    /// (`older`/`newer`/`at_or_before`/`at_or_after`) — shared by `g-`/`g+`
    /// and `:earlier`/`:later`.
    fn apply_undo_nav_result(&mut self, result: Option<(String, Cursor)>) -> Option<Cursor> {
        let (text, cursor) = result?;
        let char_len = self.buffer.len_chars();
        self.buffer.delete_range(0, char_len);
        if !text.is_empty() {
            self.buffer.insert(0, &text);
        }
        self.update_syntax();
        Some(cursor)
    }

    /// `g-`/`:earlier` (single step): move to the chronologically previous
    /// buffer state, crossing branches if necessary. Unlike `undo`, this can
    /// reach a state a plain `u` + new edit would have made unreachable.
    pub fn undo_older(&mut self) -> Option<Cursor> {
        let result = self.undo_tree.older();
        self.apply_undo_nav_result(result)
    }

    /// `g+`/`:later` (single step): move to the chronologically next buffer
    /// state.
    pub fn undo_newer(&mut self) -> Option<Cursor> {
        let result = self.undo_tree.newer();
        self.apply_undo_nav_result(result)
    }

    /// `:earlier {N}[smhd]`: jump directly to the newest state at or before
    /// `cutoff`.
    pub fn undo_at_or_before(&mut self, cutoff: SystemTime) -> Option<Cursor> {
        let result = self.undo_tree.at_or_before(cutoff);
        self.apply_undo_nav_result(result)
    }

    /// `:later {N}[smhd]`: jump directly to the oldest state at or after
    /// `cutoff`.
    pub fn undo_at_or_after(&mut self, cutoff: SystemTime) -> Option<Cursor> {
        let result = self.undo_tree.at_or_after(cutoff);
        self.apply_undo_nav_result(result)
    }

    /// `(position, total)` of the current state within the chronological
    /// (branch-crossing) list `g-`/`g+`/`:undolist` walk — 1-based position,
    /// like Vim's `:undolist`/`g-` status message.
    pub fn undo_position(&self) -> (usize, usize) {
        self.undo_tree.position()
    }

    /// `:undojoin` and the internal "these sub-command undo steps are one
    /// Vim-visible step" merge — see [`UndoTree::merge_since`]. `mark` is a
    /// `seq` previously captured via [`Self::undo_seq`].
    pub fn merge_undo_since(&mut self, mark: usize) {
        self.undo_tree.merge_since(mark);
    }

    /// `seq` of the undo step `:undojoin` should fold the *next* committed
    /// change into — the parent of whatever node the buffer currently sits
    /// on. `None` at the root (`:undojoin` errors in that case: `:h E790`).
    pub fn undo_parent_seq(&self) -> Option<usize> {
        self.undo_tree.parent_seq()
    }

    /// Discard all undo history — used by `:e!`/file-reload and by
    /// applying an on-disk replace result that bypassed the undo tree
    /// entirely (both cases: the new content has no relationship to any
    /// recorded state, so keeping stale nodes around would let `u`/`g-`
    /// "restore" text that no longer corresponds to anything on disk).
    pub fn reset_undo_history(&mut self) {
        self.undo_tree = UndoTree::new();
        self.current_undo_group = None;
    }

    /// Check if undo is available.
    pub fn can_undo(&self) -> bool {
        self.undo_tree
            .nodes
            .get(self.undo_tree.current)
            .is_some_and(|n| n.parent.is_some())
            || self.current_undo_group.as_ref().is_some_and(|g| g.dirty)
    }

    /// Check if the buffer content matches the last-saved state, based on undo tree position.
    pub fn is_at_saved_state(&self) -> bool {
        match self.saved_undo_seq {
            Some(seq) => self.undo_tree.current_seq() == seq && self.current_undo_group.is_none(),
            // Never saved: consider clean only if no edits at all.
            None => self.undo_tree.current_seq() == 0 && self.current_undo_group.is_none(),
        }
    }

    /// Check if redo is available.
    pub fn can_redo(&self) -> bool {
        self.undo_tree.can_redo()
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

        self.finish_undo_group(cursor);
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
