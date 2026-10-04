use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use super::buffer::{read_file_to_string, Buffer, BufferId};
use super::cursor::Cursor;
use super::syntax::{Syntax, SyntaxReparseOutcome};

/// Binds a scratch buffer to a provider document (#524): the id (if the
/// document already exists on the provider — `None` for the new-document
/// flow) plus the write/follow-up argv templates captured at open time, so
/// `:w` can push edits back without re-resolving the provider (which may
/// have been reconfigured or uninstalled since the buffer was opened).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolDocumentBinding {
    pub id: Option<String>,
    pub write_command: Vec<String>,
    pub write_follow_up: Vec<String>,
}

/// Binds a scratch buffer to a review-verdict-in-progress (#526): the
/// reviewed card's id plus the provider's declared verdict-command argv
/// template (still carrying its `{id}`/`{body_file}` tokens, substituted
/// at save time) — same "capture at open time so `:w` doesn't need to
/// re-resolve a provider that may have been reconfigured since" precedent
/// as [`ToolDocumentBinding`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewVerdictBinding {
    pub card_id: String,
    pub verdict_command: Vec<String>,
}

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
    ///
    /// #1280: returns the target node's `cursor_before` (where *its own*
    /// edit started), not `cursor_after` (where it finished) — matching
    /// `undo()` just above, and verified against Neovim: `g-`/`g+` restore
    /// the cursor to the start of the change the same way plain `u` does,
    /// regardless of which direction you're navigating from. Landing on a
    /// state via `cursor_after` instead only coincidentally matched the one
    /// case this had a corpus test for (`undo:g- crosses a branch abandoned
    /// by u then edit`, #1156) — that case's final state is the tree root,
    /// where `cursor_before == cursor_after` by construction — so a
    /// non-root landing (as the new `g:g+` case here does) was never
    /// exercised until now.
    pub fn older(&mut self) -> Option<(String, Cursor)> {
        let cur_seq = self.current_seq();
        let idx = self
            .live_indices_sorted()
            .into_iter()
            .rfind(|&i| self.nodes[i].seq < cur_seq)?;
        self.current = idx;
        let n = &self.nodes[idx];
        Some((n.text.clone(), n.cursor_before))
    }

    /// `g+`/`:later`: move to the live node with the next-higher `seq`,
    /// globally. See `older`, just above, for why this reads `cursor_before`.
    pub fn newer(&mut self) -> Option<(String, Cursor)> {
        let cur_seq = self.current_seq();
        let idx = self
            .live_indices_sorted()
            .into_iter()
            .find(|&i| self.nodes[i].seq > cur_seq)?;
        self.current = idx;
        let n = &self.nodes[idx];
        Some((n.text.clone(), n.cursor_before))
    }

    /// Move to the live node with the largest `seq` whose `timestamp <=
    /// cutoff` (`:earlier {N}[smhd]`). Falls back to the oldest live node if
    /// every one postdates `cutoff`.
    ///
    /// #1294 (#1280 follow-up): returns the target node's `cursor_before`,
    /// not `cursor_after` — same fix and same reasoning as `older`/`newer`,
    /// just above. Verified by hand against a live `nvim --headless`
    /// `--listen`/`--remote-send` session (not replayable through this
    /// suite's key-replay oracle harness, since `:sleep` is unimplemented in
    /// vimcode and the whole point here is a real wall-clock gap between two
    /// commits): `ihello<Esc>`, a real ~2s pause, then `0ix<Esc>` from a
    /// fresh cursor position, then `:earlier 1s` landed on the `"hello"`
    /// node with the cursor at col 0 — where the `hello` edit *started*
    /// (`cursor_before`), not col 4 where it *finished* (`cursor_after`).
    /// The forward direction (`:later {N}[smhd]`) was checked the same way
    /// and also lands on `cursor_before`.
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
        Some((n.text.clone(), n.cursor_before))
    }

    /// Move to the live node with the smallest `seq` whose `timestamp >=
    /// cutoff` (`:later {N}[smhd]`). See `at_or_before`, just above, for why
    /// this reads `cursor_before`.
    pub fn at_or_after(&mut self, cutoff: SystemTime) -> Option<(String, Cursor)> {
        let live = self.live_indices_sorted();
        let idx = live
            .iter()
            .copied()
            .find(|&i| self.nodes[i].timestamp >= cutoff)
            .or_else(|| live.last().copied())?;
        self.current = idx;
        let n = &self.nodes[idx];
        Some((n.text.clone(), n.cursor_before))
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

    /// Live nodes, oldest first, in the shape `vimcode.undo.tree` (#1654 P6)
    /// hands to a plugin: `parent` is the parent's **`seq`**, not its arena
    /// index — a plugin has no business knowing this crate's arena layout,
    /// and `seq` is the portable handle [`Self::jump_to_seq`] (and
    /// `vimcode.undo.jump`) consumes.
    pub fn tree_for_plugin(&self) -> Vec<UndoTreeNode> {
        let current = self.current;
        self.live_indices_sorted()
            .into_iter()
            .map(|i| {
                let n = &self.nodes[i];
                UndoTreeNode {
                    seq: n.seq,
                    parent: n.parent.map(|p| self.nodes[p].seq),
                    time: n.timestamp,
                    current: i == current,
                }
            })
            .collect()
    }

    /// `vimcode.undo.jump(buf, seq)` (#1654 P6): go straight to the live
    /// node carrying `seq`, unlike [`Self::older`]/[`Self::newer`] which
    /// step one chronological position at a time. Lands on the node's
    /// `cursor_after` — the same convention [`Self::redo`] uses for a node
    /// reached by moving onto it directly rather than stepping through it.
    /// `None` if no live node carries `seq`.
    pub fn jump_to_seq(&mut self, seq: usize) -> Option<(String, Cursor)> {
        let idx = self.nodes.iter().position(|n| !n.removed && n.seq == seq)?;
        self.current = idx;
        let n = &self.nodes[idx];
        Some((n.text.clone(), n.cursor_after))
    }
}

/// One undo-tree node as handed to a plugin by `vimcode.undo.tree` (#1654
/// P6) — a flattened, arena-index-free view of [`UndoNode`]. See
/// [`UndoTree::tree_for_plugin`].
#[derive(Debug, Clone)]
pub struct UndoTreeNode {
    /// This node's own sequence number.
    pub seq: usize,
    /// The parent node's sequence number — `None` only for the root.
    pub parent: Option<usize>,
    /// Wall-clock time the node was created.
    pub time: SystemTime,
    /// Whether this is the node the buffer currently reflects.
    pub current: bool,
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
    /// Row index of the line that produced `max_col`. Internal
    /// bookkeeping for `update_max_col_incremental` (#1721) — lets a
    /// keystroke-sized edit tell whether it could have invalidated the
    /// cached max without rescanning every other line.
    max_col_line: usize,
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
    /// Set when this buffer is a provider document opened for editing
    /// (#524) — `:w` pushes the title/body back through the bound
    /// provider's write command instead of writing to disk. `None` for
    /// every ordinary buffer.
    pub tool_document: Option<ToolDocumentBinding>,
    /// Set when this buffer is composing a review verdict body (#526) —
    /// `:w` reports the verdict through the bound provider's verdict
    /// command instead of writing to disk. `None` for every ordinary
    /// buffer (including a `tool_document` one; the two are mutually
    /// exclusive).
    pub review_verdict: Option<ReviewVerdictBinding>,
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
    /// Set when this buffer's window hosts a `vimcode.ui.register_view` view
    /// as an editor-area tab (`Engine::open_plugin_view_tab`, #1627), naming
    /// the view. `None` for every ordinary buffer. The buffer's own content
    /// is never read for such a window — `render::build_rendered_window`
    /// short-circuits before touching it, and `App::paint_editor_windows_rung`
    /// paints the view's `Form` in the window's rect instead of buffer text —
    /// so it exists only to piggyback on the pre-existing `Tab`/`Window`/
    /// `BufferId` lifecycle (open, close, split, move-between-groups) rather
    /// than inventing a parallel one; `scratch_name` (set alongside this,
    /// `Engine::open_plugin_view_tab`) is the pre-existing tab-title mechanism
    /// this reuses rather than adds a second.
    pub plugin_view: Option<String>,
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

/// Does a highlight span `[start, end)` overlap `[rehighlight_start,
/// rehighlight_end)`? Used symmetrically in [`BufferState::patch_highlights`]
/// to decide both what to drop from the cached set and what to keep from
/// the fresh re-extraction.
///
/// A zero-width span (`start == end`) gets inclusive-both-ends treatment
/// instead of the normal half-open overlap test: zero-width captures are
/// tree-sitter error-recovery "MISSING" node markers (common on text
/// that's momentarily invalid mid-edit, e.g. an unclosed `(`), and
/// tree-sitter's own `set_byte_range` already excludes one sitting exactly
/// at the half-open range's exclusive end — confirmed by direct
/// experiment. Mirroring that exclusion here instead of correcting for it
/// would silently drop a real capture the moment it landed on a boundary.
fn overlaps_rehighlight(
    start: usize,
    end: usize,
    rehighlight_start: usize,
    rehighlight_end: usize,
) -> bool {
    if start == end {
        start >= rehighlight_start && start <= rehighlight_end
    } else {
        start < rehighlight_end && end > rehighlight_start
    }
}

/// Widen `[start, end)` (new-text coordinates) to swallow any entry in
/// `highlights` (sorted by start byte, old-text coordinates) that touches
/// it exactly — `*e == start` on the left, or `*s == end` (converted to
/// old-text coordinates first) on the right — repeating until a pass
/// makes no further change (a touched entry's own far boundary can in
/// turn touch another entry).
///
/// Needed because `Syntax::reparse_incremental`'s `changed_ranges`-
/// derived window can under-report when an edit lands exactly on an old
/// node's boundary: observed in practice, an insertion immediately after
/// an identifier extended that identifier in the new tree (a `property`
/// capture growing from `(38, 39)` to `(38, 41)`), with `changed_ranges`
/// reporting nothing before the insertion point — tree-sitter's own
/// boundary-adjustment heuristic guessed the inserted text was outside
/// the node, and nothing corrected that guess before the comparison.
///
/// A free function taking `highlights: &[...]` rather than a
/// `BufferState` method taking `&self`: its caller
/// (`update_syntax_with_limit`) already holds `self.syntax` borrowed
/// mutably across the whole match arm, and a `&self` method call would
/// borrow all of `self`, conflicting with that — a plain
/// `&self.highlights` field borrow at the call site doesn't. Binary
/// search instead of a linear scan for the same reason this whole module
/// exists: `highlights` holds every highlight in the file (not just the
/// viewport), and this runs on every keystroke, so an O(file) scan here
/// would silently reintroduce the per-keystroke cost being removed.
fn widen_rehighlight_to_touching_entries(
    highlights: &[(usize, usize, String)],
    edit: tree_sitter::InputEdit,
    mut start: usize,
    mut end: usize,
) -> (usize, usize) {
    let delta = edit.new_end_byte as i64 - edit.old_end_byte as i64;
    loop {
        let mut changed = false;

        // `start` is always within the shared "before the edit"
        // coordinate space (nothing before `edit.start_byte` ever
        // changes, and `start` never exceeds that), so it compares
        // directly against cached (old-coordinate) entries. Sorted by
        // start byte, so only the entry immediately before the first one
        // with `start_byte >= start` can touch it from the left.
        let idx = highlights.partition_point(|h| h.0 < start);
        if idx > 0 {
            let (s, e, _) = &highlights[idx - 1];
            if *e == start && *s < start {
                start = *s;
                changed = true;
            }
        }

        // `end` is a new-text coordinate; convert to the equivalent
        // old-text coordinate before comparing against cached entries,
        // then convert any found extension back.
        let end_in_old = (end as i64 - delta).max(0) as usize;
        let idx = highlights.partition_point(|h| h.0 < end_in_old);
        if let Some((s, e, _)) = highlights.get(idx) {
            if *s == end_in_old && *e > end_in_old {
                end = (*e as i64 + delta).max(0) as usize;
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
    (start, end)
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
            max_col_line: 0,
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
            tool_document: None,
            review_verdict: None,
            diff_label: None,
            file_mtime: None,
            file_change_warned: false,
            detected_indent: None,
            line_ending: LineEnding::LF,
            plugin_view: None,
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
            max_col_line: 0,
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
            tool_document: None,
            review_verdict: None,
            diff_label: None,
            file_mtime,
            file_change_warned: false,
            detected_indent: None,
            line_ending,
            plugin_view: None,
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
    ///
    /// Called on every keystroke (#1721), so the three expensive parts are
    /// all either incremental or skipped when nothing changed:
    /// `Syntax::reparse_incremental` reuses the previous parse tree instead
    /// of a full reparse, `patch_highlights` re-extracts highlights only
    /// for the byte range that actually changed instead of the whole file,
    /// and `max_col` is updated from just the touched lines unless that
    /// can't prove the global max (in which case it falls back to a full
    /// rescan, same as before).
    pub fn update_syntax_with_limit(&mut self, max_lines: usize) {
        let text = self.buffer.to_string();
        let over_limit = self.buffer.content.len_lines() > max_lines;
        if over_limit {
            self.highlights = Vec::new();
            self.rescan_max_col(&text);
            return;
        }
        let Some(syn) = self.syntax.as_mut() else {
            self.highlights = Vec::new();
            self.rescan_max_col(&text);
            return;
        };
        match syn.reparse_incremental(&text) {
            SyntaxReparseOutcome::Unchanged => {
                // Text didn't change since the last call (common — many
                // call sites call `update_syntax` defensively) — nothing to
                // re-extract or rescan.
            }
            SyntaxReparseOutcome::Full => {
                let mut hl = syn.extract_highlights(&text);
                // Ensure sorted by start_byte — the render pipeline uses
                // binary search (partition_point) to narrow highlights to
                // the viewport.
                hl.sort_by_key(|h| h.0);
                self.highlights = hl;
                self.rescan_max_col(&text);
            }
            SyntaxReparseOutcome::Incremental {
                edit,
                rehighlight_start,
                rehighlight_end,
            } => {
                // `changed_ranges` can under-report when an edit lands
                // exactly on an old node's boundary (observed in
                // practice: inserting text immediately after an
                // identifier extended that identifier in the new tree,
                // with `changed_ranges` reporting nothing before the
                // insertion point) — widen to swallow any cached entry
                // that touches the window exactly before trusting it.
                let (rehighlight_start, rehighlight_end) = widen_rehighlight_to_touching_entries(
                    &self.highlights,
                    edit,
                    rehighlight_start,
                    rehighlight_end,
                );
                // Pad the queried range by 1 byte on each side: tree-
                // sitter's `set_byte_range` excludes a *zero-width*
                // capture (an error-recovery "MISSING" node, common on
                // text that's momentarily invalid mid-edit) sitting
                // exactly at the exclusive end boundary — confirmed by
                // direct experiment, not just inference — and by
                // symmetry potentially the start boundary too. Deliberately
                // *not* clamped to `text.len()` on the high side: a
                // zero-width node can sit exactly at EOF (an unclosed
                // `{`/`(` while typing is common, and tree-sitter inserts
                // its MISSING-token marker right at the end of the file),
                // and querying `set_byte_range(_, text.len())` alone still
                // excludes it by that same exclusive-boundary rule — only
                // `text.len() + 1` reaches it. Confirmed safe to query one
                // byte past the real length (no panic, no out-of-bounds
                // read — `set_byte_range` only stores the bound for
                // filtering, text access stays governed by the actual
                // nodes). `patch_highlights` re-applies the *unpadded*
                // bounds itself (via `overlaps_rehighlight`) so a padding-
                // only capture that isn't actually a boundary marker
                // doesn't get kept twice.
                let fetch_start = rehighlight_start.saturating_sub(1);
                let fetch_end = rehighlight_end + 1;
                let fresh = syn.extract_highlights_range(&text, fetch_start, fetch_end);
                self.patch_highlights(edit, rehighlight_start, rehighlight_end, fresh);
                self.update_max_col_incremental(&text, edit);
            }
        }
    }

    /// Splice freshly-extracted highlights for `[rehighlight_start,
    /// rehighlight_end)` (new-text byte offsets) into the existing cached
    /// `self.highlights`, instead of re-extracting the whole file (#1721).
    ///
    /// Cached entries are bare byte-range tuples in *old*-text
    /// coordinates, so they're first partitioned in that coordinate
    /// space: entries entirely before `edit.start_byte` are untouched
    /// (old and new text are identical up to there), entries entirely at
    /// or after `edit.old_end_byte` are shifted to new-text coordinates
    /// by the edit's length delta (`InputEdit::edit_range`-equivalent,
    /// done by hand since highlights aren't `tree_sitter::Range`s), and
    /// everything else — inside the edit's old span — is dropped outright
    /// as stale. *Only after* that remapping are the survivors (now in
    /// new-text coordinates) compared against `[rehighlight_start,
    /// rehighlight_end)` (also new-text coordinates, can be wider than
    /// the edit itself — `changed_ranges` may extend past it) and
    /// anything overlapping is dropped too, to be replaced by `fresh`.
    /// Comparing old-coordinate entries directly against
    /// `rehighlight_start`/`rehighlight_end` without this first pass is
    /// wrong whenever the edit changes length (`old_end_byte !=
    /// new_end_byte`) — the two coordinate spaces disagree past the edit
    /// point, and a stale in-old-span entry can slip through if it
    /// happens to land outside the new-coordinate window by sheer
    /// numeric coincidence.
    ///
    /// `fresh` itself is filtered to captures that actually overlap the
    /// span: tree-sitter's `set_byte_range` restricts which *nodes* get
    /// traversed by overlap with the queried range, not which individual
    /// capture spans are reported — a capture on a small sibling of a node
    /// that merely encloses the range can "leak" through attached to the
    /// same match, with a span nowhere near the range (observed in
    /// practice: a `let` binding's identifier captured as `@function`
    /// leaking in from several bytes before the query's start). Such a
    /// leaked capture duplicates content already present in the kept set,
    /// so it's dropped here rather than trusted to widen the patched
    /// region.
    fn patch_highlights(
        &mut self,
        edit: tree_sitter::InputEdit,
        rehighlight_start: usize,
        rehighlight_end: usize,
        fresh: Vec<(usize, usize, String)>,
    ) {
        let delta = edit.new_end_byte as i64 - edit.old_end_byte as i64;
        let mut kept: Vec<(usize, usize, String)> = self
            .highlights
            .drain(..)
            .filter_map(|(start, end, name)| {
                if end <= edit.start_byte {
                    // Entirely before the edit (old-text coordinates) —
                    // old and new text are identical up to `start_byte`,
                    // so these byte offsets are unaffected.
                    Some((start, end, name))
                } else if start >= edit.old_end_byte {
                    // Entirely at or after the edit (old-text
                    // coordinates) — shift to new-text coordinates by
                    // the length delta.
                    let new_start = (start as i64 + delta).max(0) as usize;
                    let new_end = (end as i64 + delta).max(0) as usize;
                    Some((new_start, new_end, name))
                } else {
                    // Overlaps the edit's old span — stale, dropped.
                    None
                }
            })
            // Survivors are now in new-text coordinates (or coordinates
            // valid in both spaces, for the before-edit case). Drop any
            // that additionally fall inside the wider rehighlight window.
            .filter(|(start, end, _)| {
                !overlaps_rehighlight(*start, *end, rehighlight_start, rehighlight_end)
            })
            .collect();

        kept.extend(fresh.into_iter().filter(|(start, end, _)| {
            overlaps_rehighlight(*start, *end, rehighlight_start, rehighlight_end)
        }));
        kept.sort_by_key(|h| h.0);
        self.highlights = kept;
    }

    /// Update `self.max_col` from just the lines the edit touched, instead
    /// of rescanning every line (#1721).
    ///
    /// Safe without reading any line outside the touched span because
    /// `self.max_col_line` remembers *which* line last produced the cached
    /// max: if the edit didn't touch that line, every line outside the
    /// touched span is provably unchanged, so the old max still holds
    /// unless the touched span now exceeds it. The only case this can't
    /// resolve locally is the edit touching `max_col_line` itself without
    /// growing past the old max — that line may have shrunk, and nothing
    /// here tracks the length of every other line to know what the new
    /// true max is, so it falls back to a full rescan (same cost as
    /// before #1721, just no longer paid on every keystroke).
    ///
    /// `max_col_line` is a row index, and rows are old/new-text-coordinate
    /// sensitive exactly like highlight byte offsets are: an edit that
    /// adds or removes a line *anywhere before* `max_col_line` shifts
    /// every row number after it, the same way an edit shifts byte
    /// offsets after it in `patch_highlights`. Forgetting to shift it
    /// (confirmed by the randomised oracle test below, not just
    /// inference) leaves `max_col_line` pointing at the wrong row —
    /// harmless until some *other*, later edit shrinks whatever line now
    /// actually occupies that row, which this function then wrongly
    /// trusts as "the max line wasn't touched" and leaves `max_col`
    /// stuck too high.
    fn update_max_col_incremental(&mut self, text: &str, edit: tree_sitter::InputEdit) {
        // The `&str` range-indexing below panics on an offset that isn't a
        // UTF-8 char boundary. `compute_edit` guarantees both offsets are
        // (by snapping them *outwards* to one — see its comment); state
        // the dependency here so a future change there surfaces as this
        // named assertion rather than a bare "byte index N is not a char
        // boundary" from the middle of a max-col calculation (#1721).
        debug_assert!(
            text.is_char_boundary(edit.start_byte) && text.is_char_boundary(edit.new_end_byte),
            "InputEdit offsets must be char boundaries in the new text: \
             start_byte={}, new_end_byte={}",
            edit.start_byte,
            edit.new_end_byte
        );
        let line_start = text[..edit.start_byte]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let line_end = text[edit.new_end_byte..]
            .find('\n')
            .map(|i| edit.new_end_byte + i)
            .unwrap_or(text.len());

        let mut touched_max = 0usize;
        let mut touched_max_row_offset = 0usize;
        for (i, line) in text[line_start..line_end].split('\n').enumerate() {
            let len = line.chars().count();
            if len > touched_max {
                touched_max = len;
                touched_max_row_offset = i;
            }
        }
        let touched_start_row = edit.start_position.row;
        let touched_old_end_row = edit.old_end_position.row;
        let touched_new_end_row = edit.new_end_position.row;
        let row_delta = touched_new_end_row as i64 - touched_old_end_row as i64;
        let touched_max_line = touched_start_row + touched_max_row_offset;

        if touched_max > self.max_col {
            // Unambiguous new global max.
            self.max_col = touched_max;
            self.max_col_line = touched_max_line;
            return;
        }
        if self.max_col_line < touched_start_row {
            // Entirely before the edit (shared old/new row numbering) —
            // untouched, still valid.
            return;
        }
        if self.max_col_line > touched_old_end_row {
            // Entirely after the edit, in *old*-text row numbering — the
            // line itself is untouched content, but its row number shifts
            // by however many lines this edit added or removed.
            self.max_col_line = (self.max_col_line as i64 + row_delta).max(0) as usize;
            return;
        }
        // Inside the touched span (old-text row numbering).
        if touched_max == self.max_col {
            // The tracked max line was touched, but a line within the
            // touched span still ties the old max — re-anchor to it (any
            // line of that length re-establishes the invariant; it need
            // not be the exact original line).
            self.max_col_line = touched_max_line;
            return;
        }
        // The tracked max line was touched and nothing touched reaches
        // the old max anymore — it may have shrunk, and nothing here
        // tracks every other line's length to know the new true max.
        self.rescan_max_col(text);
    }

    /// Full O(file) scan for the longest line, and which line it is —
    /// the fallback `update_max_col_incremental` uses when it can't prove
    /// the cached max is still correct, and what every other code path
    /// (no syntax, over the highlight size limit, full reparse) uses.
    fn rescan_max_col(&mut self, text: &str) {
        let mut max_col = 0usize;
        let mut max_row = 0usize;
        for (row, line) in text.lines().enumerate() {
            let len = line.chars().count();
            if len > max_col {
                max_col = len;
                max_row = row;
            }
        }
        self.max_col = max_col;
        self.max_col_line = max_row;
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
    ///
    /// Goes through the same BOM-aware decode as `Buffer::from_file` (#1560):
    /// this is reachable from `:e[dit]!` and from the idle file-watcher's
    /// silent-reload path, and a bare `fs::read_to_string` here reproduces
    /// the exact "stream did not contain valid UTF-8" error for a
    /// UTF-16/UTF-8-BOM file that the initial open already fixed.
    pub fn reload_from_disk(&mut self) -> Result<(), io::Error> {
        if let Some(path) = self.file_path.clone() {
            let text = read_file_to_string(&path)?;
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
    /// `<C-r>` (redo) or a time-spec `:later {N}[smhd]` landing on this node.
    /// (#1280: `g+`/count-based `:later N` land on `cursor_before` instead —
    /// see `newer`'s doc comment, just below `older`'s, for why.)
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

    /// `vimcode.undo.tree(buf)` (#1654 P6) — the full live undo tree, in
    /// plugin-facing shape. See [`UndoTree::tree_for_plugin`].
    pub fn undo_tree_for_plugin(&self) -> Vec<UndoTreeNode> {
        self.undo_tree.tree_for_plugin()
    }

    /// `vimcode.undo.jump(buf, seq)` (#1654 P6) — the one mutation the P6
    /// read API ships, and it goes entirely through the existing undo
    /// machinery ([`UndoTree::jump_to_seq`]) rather than hand-rolling a
    /// buffer swap.
    pub fn undo_jump(&mut self, seq: usize) -> Option<Cursor> {
        let result = self.undo_tree.jump_to_seq(seq);
        self.apply_undo_nav_result(result)
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

    /// True if `path` (canonicalized) is already open under some `BufferId`.
    /// Mirrors the dedup check `open_file` does internally, exposed so
    /// callers can decide reuse-vs-create *before* calling it (#1298: `:edit`
    /// reusing the pristine scratch buffer must lose to an already-open
    /// buffer for the same path, matching Neovim).
    pub fn is_path_open(&self, path: &Path) -> bool {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.buffers.values().any(|state| {
            state.file_path.as_ref().is_some_and(|existing_path| {
                let existing_canonical = existing_path
                    .canonicalize()
                    .unwrap_or_else(|_| existing_path.clone());
                existing_canonical == canonical
            })
        })
    }

    /// Build the `BufferState` for opening `path` into buffer `id` — shared
    /// by [`open_file`](Self::open_file) (fresh id) and
    /// [`reopen_buffer`](Self::reopen_buffer) (reused id).
    fn load_buffer_state(id: BufferId, path: &Path) -> Result<BufferState, io::Error> {
        if path.exists() {
            let buffer = Buffer::from_file(id, path)?;
            Ok(BufferState::with_file(buffer, path.to_path_buf()))
        } else {
            // New file (doesn't exist yet)
            let buffer = Buffer::new(id);
            Ok(BufferState::with_file(buffer, path.to_path_buf()))
        }
    }

    /// Create a buffer from a file. Reuses existing buffer if file is already open.
    pub fn open_file(&mut self, path: &Path) -> Result<BufferId, io::Error> {
        // Check if file is already open
        if let Some(id) = self.buffers.iter().find_map(|(id, state)| {
            let existing_path = state.file_path.as_ref()?;
            let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            let existing_canonical = existing_path
                .canonicalize()
                .unwrap_or_else(|_| existing_path.clone());
            (existing_canonical == canonical).then_some(*id)
        }) {
            return Ok(id);
        }

        // Create new buffer
        let id = BufferId(self.next_id);
        self.next_id += 1;

        let buffer_state = Self::load_buffer_state(id, path)?;
        self.buffers.insert(id, buffer_state);
        self.add_recent_file(path);
        Ok(id)
    }

    /// Rewrite `id`'s buffer in place to open `path`, keeping the same
    /// `BufferId` instead of allocating a new one — Neovim's `:edit` reuses
    /// the still-pristine startup scratch buffer this way rather than
    /// leaving it behind as a numbered phantom (#1298). Caller
    /// (`Engine::open_file_with_mode_impl`) is responsible for checking that
    /// `id` is actually still-pristine and that `path` isn't already open
    /// under a different id; this just performs the in-place replace.
    pub fn reopen_buffer(&mut self, id: BufferId, path: &Path) -> Result<(), io::Error> {
        let buffer_state = Self::load_buffer_state(id, path)?;
        self.buffers.insert(id, buffer_state);
        self.add_recent_file(path);
        Ok(())
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

    /// #1721 integration test: `update_syntax_with_limit`'s incremental
    /// path (`patch_highlights` + `update_max_col_incremental`) must agree
    /// with a from-scratch full parse / full line-length rescan after
    /// every edit — not just on average, exactly. The sequence below
    /// deliberately drives every branch `update_max_col_incremental` has:
    /// a touched line growing past the cached max (fast path, no touched
    /// line relationship to the old max), an edit to an unrelated line
    /// (fast path, cached max untouched), the max-holding line shrinking
    /// back down (forces the full-rescan fallback), and a brand new line
    /// elsewhere becoming the max.
    #[test]
    fn test_update_syntax_with_limit_incremental_matches_full_rescan() {
        use crate::core::syntax::{Syntax, SyntaxLanguage};

        fn assert_matches_full_rescan(state: &mut BufferState) {
            let text = state.buffer.to_string();

            let mut fresh = Syntax::new_for_language(SyntaxLanguage::Rust);
            let mut want = fresh.parse(&text);
            want.sort_by_key(|h| h.0);
            let mut got = state.highlights.clone();
            got.sort_by_key(|h| h.0);
            assert_eq!(got, want, "highlights diverged from a full rescan:\n{text}");

            let want_max = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
            assert_eq!(
                state.max_col, want_max,
                "max_col diverged from a full rescan:\n{text}"
            );
        }

        let mut state = BufferState::new(Buffer::new(crate::core::buffer::BufferId(0)));
        state.syntax = Some(Syntax::new_for_language(SyntaxLanguage::Rust));
        state.buffer.insert(
            0,
            "fn main() {\n    let x = 1;\n    println!(\"{}\", x);\n}\n",
        );
        state.update_syntax_with_limit(usize::MAX);
        assert_matches_full_rescan(&mut state);

        // 1. Lengthen the `let x` line until it becomes the longest line
        //    in the file.
        let pos = state.buffer.to_string().find("let ").unwrap() + "let ".len();
        state
            .buffer
            .insert(pos, "really_long_identifier_name_0123456789_");
        state.update_syntax_with_limit(usize::MAX);
        assert_matches_full_rescan(&mut state);

        // 2. Edit an unrelated (non-max) line — must not disturb the
        //    cached max, and the touched span's highlights must still
        //    match a full parse.
        let pos = state.buffer.to_string().find("println!").unwrap();
        state.buffer.insert(pos, "/* note */ ");
        state.update_syntax_with_limit(usize::MAX);
        assert_matches_full_rescan(&mut state);

        // 3. Shrink the line that currently holds the max back down —
        //    forces the full-rescan fallback, since the touched line
        //    *was* the max and no longer grows past it.
        let text = state.buffer.to_string();
        let start = text
            .find("really_long_identifier_name_0123456789_")
            .unwrap();
        let end = start + "really_long_identifier_name_0123456789_".len();
        state.buffer.delete_range(start, end);
        state.update_syntax_with_limit(usize::MAX);
        assert_matches_full_rescan(&mut state);

        // 4. A brand new, much longer line elsewhere becomes the new
        //    global max.
        let pos = state.buffer.to_string().find("fn main").unwrap();
        state.buffer.insert(
            pos,
            "// a very very very very very very very very very long comment line\n",
        );
        state.update_syntax_with_limit(usize::MAX);
        assert_matches_full_rescan(&mut state);
    }

    /// #1721 review regression: replacing a CJK character with a sibling
    /// that shares its leading UTF-8 bytes (`字` = `E5 AD 97`, `存` =
    /// `E5 AD 98`) through the real buffer-mutation path must not panic.
    /// `compute_edit`'s byte-level common-prefix/suffix scan used to stop
    /// mid-codepoint for exactly this pair, producing an `InputEdit` whose
    /// `start_byte`/`new_end_byte` weren't char boundaries — the very next
    /// use of them, `update_max_col_incremental`'s `&str` range-indexing,
    /// panicked with "byte index N is not a char boundary" on every such
    /// keystroke (an everyday CJK `r`-replace or IME correction, not an
    /// adversarial case).
    #[test]
    fn test_update_syntax_with_limit_cjk_shared_prefix_substitution_no_panic() {
        use crate::core::syntax::{Syntax, SyntaxLanguage};

        let mut state = BufferState::new(Buffer::new(crate::core::buffer::BufferId(0)));
        state.syntax = Some(Syntax::new_for_language(SyntaxLanguage::Rust));
        state.buffer.insert(0, "let x = \"字\";\n");
        state.update_syntax_with_limit(usize::MAX);

        // `Buffer::insert`/`delete_range` take *char* indices, so locate
        // the character by char position (not `str::find`'s byte offset).
        let text = state.buffer.to_string();
        let start = text.chars().position(|c| c == '字').unwrap();
        state.buffer.delete_range(start, start + 1);
        state.buffer.insert(start, "存");
        state.update_syntax_with_limit(usize::MAX);

        let text = state.buffer.to_string();
        let want_max = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
        assert_eq!(state.max_col, want_max);
    }

    /// #1721 review round 2 regression: the *suffix*-side counterpart of
    /// the test above. 'җ' (U+0497, `D2 97`) and '字' (U+5B57,
    /// `E5 AD 97`) share their *trailing* byte and have *different* byte
    /// lengths, so `compute_edit`'s byte-level common-*suffix* scan stops
    /// mid-codepoint at a different depth in each text.
    ///
    /// The first fix snapped `old_end_byte` inwards against the old text
    /// and transferred the same numeric delta to `new_end_byte`, which
    /// left `new_end_byte` on a continuation byte of the new text for the
    /// `җ` → `字` direction — so `update_max_col_incremental`'s
    /// `text[edit.new_end_byte..]` panicked with "byte index N is not a
    /// char boundary", exactly the crash class the round-1 fix was meant
    /// to close. Both substitution directions are driven here, at the end
    /// of the buffer (nothing following to force a safe ASCII stop).
    #[test]
    fn test_update_syntax_with_limit_cjk_shared_suffix_substitution_no_panic() {
        use crate::core::syntax::{Syntax, SyntaxLanguage};

        for (first, second) in [('җ', '字'), ('字', 'җ')] {
            let mut state = BufferState::new(Buffer::new(crate::core::buffer::BufferId(0)));
            state.syntax = Some(Syntax::new_for_language(SyntaxLanguage::Rust));
            state.buffer.insert(0, &format!("let x = 1; // {first}"));
            state.update_syntax_with_limit(usize::MAX);

            // `Buffer::insert`/`delete_range` take *char* indices.
            let text = state.buffer.to_string();
            let start = text.chars().position(|c| c == first).unwrap();
            state.buffer.delete_range(start, start + 1);
            state.buffer.insert(start, &second.to_string());
            state.update_syntax_with_limit(usize::MAX);

            let text = state.buffer.to_string();
            let want_max = text.lines().map(|l| l.chars().count()).max().unwrap_or(0);
            assert_eq!(
                state.max_col, want_max,
                "{first} -> {second}: max_col diverged from a full rescan"
            );
        }
    }

    /// #1721 oracle, randomised: a long sequence of random inserts/deletes
    /// (multi-byte UTF-8, newlines, random positions) plus simulated
    /// undo/redo, driven through the *real* `update_syntax_with_limit`
    /// path (not `Syntax` directly — this is what caught a real bug the
    /// hand-written scenario above didn't: `extract_highlights_range`
    /// can return a capture whose own span lies outside the queried byte
    /// range, entirely because an enclosing node happened to overlap it;
    /// trusting that capture to widen the "drop zone" discarded unrelated,
    /// still-valid cached highlights). After every step, `highlights` and
    /// `max_col` must match a from-scratch full parse / full rescan.
    #[test]
    fn test_update_syntax_with_limit_fuzz_matches_full_rescan() {
        use crate::core::syntax::{Syntax, SyntaxLanguage};

        struct Lcg(u64);
        impl Lcg {
            fn next_u64(&mut self) -> u64 {
                self.0 = self
                    .0
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                self.0
            }
            fn next_usize(&mut self, bound: usize) -> usize {
                if bound == 0 {
                    0
                } else {
                    (self.next_u64() % bound as u64) as usize
                }
            }
        }

        fn apply_random_edit(chars: &mut Vec<char>, rng: &mut Lcg) {
            // Two deliberately-colliding pairs (#1721 review), because
            // every other char in the original list had a distinct
            // leading *and* trailing byte from every other, which made
            // both classes of bug invisible to the fuzz corpus:
            //
            // * '字' (U+5B57, `E5 AD 97`) / '存' (U+5B58, `E5 AD 98`)
            //   share their *leading* two bytes, so the common-*prefix*
            //   scan stops one byte short of the full character.
            // * 'җ' (U+0497, `D2 97`) / '字' (U+5B57, `E5 AD 97`) share
            //   their *trailing* byte and differ in byte *length*, so the
            //   common-*suffix* scan stops mid-codepoint at a different
            //   depth in each text.
            //
            // Either lands an `InputEdit` boundary off a char boundary
            // unless `compute_edit` snaps it, and the very next use of
            // those offsets is `update_max_col_incremental`'s `&str`
            // range-indexing, which panics on a non-boundary index.
            const CHOICES: &[char] = &[
                'a', 'b', 'c', '_', '(', ')', '{', '}', ';', ' ', '\n', '"', 'é', 'λ', '字', '存',
                'җ', '🙂',
            ];
            let len = chars.len();
            // In-place *substitution* (#1721 review round 2) is what makes
            // the colliding pairs above actually collide: an insert or
            // delete shifts everything after it, so the byte-level
            // common-prefix/suffix scan meets its first differing byte on
            // a character boundary almost every time. Only replacing one
            // character with another pits two encodings against each
            // other byte-for-byte.
            //
            // And the substitution is *biased* towards the colliding
            // partner rather than left to chance: hitting a pair randomly
            // needs the chosen position to already hold one of them and
            // the random replacement to be its exact partner, a
            // ~0.1%-per-step coincidence that (measured against the
            // round-1-broken `compute_edit`) 300 steps did not produce.
            let partner = |c: char, rng: &mut Lcg| match c {
                // '字' collides on its leading bytes with '存' and on its
                // trailing byte with 'җ' — pick either at random.
                '字' => Some(if rng.next_usize(2) == 0 { '存' } else { 'җ' }),
                '存' | 'җ' => Some('字'),
                _ => None,
            };
            let roll = rng.next_usize(100);
            if len > 0 && roll < 25 {
                let pos = rng.next_usize(len);
                chars[pos] = partner(chars[pos], rng)
                    .unwrap_or_else(|| CHOICES[rng.next_usize(CHOICES.len())]);
            } else if len == 0 || roll < 70 {
                let pos = rng.next_usize(len + 1);
                let c = CHOICES[rng.next_usize(CHOICES.len())];
                chars.insert(pos, c);
            } else {
                let pos = rng.next_usize(len);
                let max_del = (len - pos).clamp(1, 4);
                let del_len = 1 + rng.next_usize(max_del);
                let end = (pos + del_len).min(len);
                chars.drain(pos..end);
            }
        }

        // The seed carries the UTF-8-colliding characters from the start
        // so substitutions can fire in the first few steps rather than
        // waiting for a random insert to introduce one.
        let seed = "fn main() {\n    let x = 1;\n    let s = \"字存җ\";\n    if x > 0 {\n        println!(\"{}\", x);\n    }\n}\n";
        let mut chars: Vec<char> = seed.chars().collect();

        let mut state = BufferState::new(Buffer::new(crate::core::buffer::BufferId(0)));
        state.syntax = Some(Syntax::new_for_language(SyntaxLanguage::Rust));
        state.buffer.insert(0, &chars.iter().collect::<String>());
        state.update_syntax_with_limit(usize::MAX);

        let mut history: Vec<Vec<char>> = vec![chars.clone()];
        let mut rng = Lcg(0x5EED_1234_ABCD_9876);

        for step in 0..300 {
            if step % 13 == 12 && history.len() > 1 {
                let back = 1 + rng.next_usize(history.len() - 1);
                chars = history[history.len() - 1 - back].clone();
            } else {
                apply_random_edit(&mut chars, &mut rng);
            }
            history.push(chars.clone());
            let new_text: String = chars.iter().collect();

            let old_len = state.buffer.content.len_chars();
            state.buffer.delete_range(0, old_len);
            state.buffer.insert(0, &new_text);
            state.update_syntax_with_limit(usize::MAX);

            let mut fresh = Syntax::new_for_language(SyntaxLanguage::Rust);
            let mut want = fresh.parse(&new_text);
            want.sort_by_key(|h| h.0);
            let mut got = state.highlights.clone();
            got.sort_by_key(|h| h.0);
            assert_eq!(
                got, want,
                "step {step}: highlights diverged from a full rescan\ntext:\n{new_text:?}"
            );

            let want_max = new_text
                .lines()
                .map(|l| l.chars().count())
                .max()
                .unwrap_or(0);
            assert_eq!(
                state.max_col, want_max,
                "step {step}: max_col diverged from a full rescan\ntext:\n{new_text:?}"
            );
        }
    }

    // Note: the atomic-sync path (`set_syntax_max_lines` → `update_syntax`
    // reading the global) is untested because it races with any parallel
    // test that constructs an `Engine` — `Engine::new` writes the atomic
    // from `Settings::default()`. The gate logic is covered above via
    // `update_syntax_with_limit`; the production sync is a single-line
    // `set_syntax_max_lines(...)` call in `Engine::new` and `set_value_str`.

    /// #1560 (review finding): `reload_from_disk` — the sibling read path
    /// used by `:e[dit]!` and the idle file-watcher's silent-reload — has
    /// its own `fs::read_to_string` call that reproduces the exact "stream
    /// did not contain valid UTF-8" error for a UTF-16LE-BOM file. This
    /// exercises reload specifically (not `Buffer::from_file`/initial open,
    /// already covered in `buffer.rs`) to prove the fix reaches both reads:
    /// open the file as plain UTF-8 first, then externally rewrite it as
    /// UTF-16LE-with-BOM and reload — the new content must come through
    /// decoded, not error out.
    #[test]
    fn test_reload_from_disk_utf16le_bom() {
        let path = std::env::temp_dir().join("vimcode_buffer_manager_test_reload_utf16le.txt");
        std::fs::write(&path, "old content").unwrap();

        let buffer = Buffer::from_file(crate::core::buffer::BufferId(0), &path).unwrap();
        let mut state = BufferState::with_file(buffer, path.clone());
        assert_eq!(state.buffer.to_string(), "old content");

        let mut bytes: Vec<u8> = vec![0xFF, 0xFE]; // UTF-16LE BOM
        for unit in "new content".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        std::fs::write(&path, &bytes).unwrap();

        state.reload_from_disk().unwrap();
        assert_eq!(state.buffer.to_string(), "new content");

        let _ = std::fs::remove_file(&path);
    }
}
