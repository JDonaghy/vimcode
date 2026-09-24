//! The change-review surface (#955, shared with #525): a source-agnostic
//! list of proposed file changes, rendered through `quadraui::DiffView`.
//!
//! Neither #955 (ACP tool-call diffs) nor #525 (git branch diff review)
//! owns this — whichever lands first builds it, the other consumes it.
//! #955 lands it: [`ProposedChange`] carries nothing ACP-specific (just
//! `path`/`old_text`/`new_text`), so a git-derived feeder for #525
//! constructs the exact same struct from `git diff` output instead of a
//! `session/update` `diff` content block. See this module's
//! `builds_from_a_hand_rolled_non_acp_change_list` test for a non-ACP feed
//! proving the point.
//!
//! This module is deliberately free of any `Engine`/buffer/filesystem
//! knowledge — `crate::core::engine::review_ops` is the only bridge
//! between this pure data model and the rest of the editor (opening the
//! surface, keyboard handling, applying an accepted change to a real
//! buffer, click-to-jump).

use quadraui::{DiffEditability, DiffMode, DiffPane, DiffView};

/// One proposed change to a single file — source-agnostic: an ACP `diff`
/// content block and a git-branch diff both construct this the same way.
/// `old_text: None` marks a new file (no left side at all, per the ACP v1
/// schema's `oldText: string | null`) — distinct from `Some(String::new())`
/// (an existing, empty file), though [`ChangeReviewEntry::new`] diffs both
/// the same way (`compute_hunks` against `""`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedChange {
    pub path: String,
    pub old_text: Option<String>,
    pub new_text: String,
}

/// Human decision on one [`ChangeReviewEntry`]. `Pending` is the only
/// state hunk navigation is meaningful against; accepting or rejecting is
/// terminal for that entry within this review session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeDecision {
    Pending,
    Accepted,
    Rejected,
}

/// Build a single hunk where every line of `text` is a pure `Added` row
/// (`left: None`) — the "new file" branch of [`ChangeReviewEntry::new`].
/// A single trailing empty element from `text.split('\n')` (the normal
/// shape of a file ending in one newline) is dropped so a file ending in
/// `\n` renders its real line count, not one extra blank "added" row.
/// Returns no hunks at all for a genuinely empty new file.
fn pure_addition_hunks(text: &str) -> Vec<quadraui::DiffHunk> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.len() > 1 && lines.last() == Some(&"") {
        lines.pop();
    }
    vec![quadraui::DiffHunk {
        left_start: 1,
        right_start: 1,
        rows: lines
            .into_iter()
            .map(|line| quadraui::DiffRow {
                left: None,
                right: Some(line.to_string()),
                kind: quadraui::DiffRowKind::Added,
            })
            .collect(),
    }]
}

/// One file's entry in a [`ChangeReviewState`]: the source change, the
/// pre-computed [`DiffView`] built from it, and the human's decision so
/// far.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeReviewEntry {
    pub change: ProposedChange,
    pub view: DiffView,
    pub decision: ChangeDecision,
}

impl ChangeReviewEntry {
    fn new(change: ProposedChange, id_suffix: usize) -> Self {
        let is_new_file = change.old_text.is_none();
        let left = change.old_text.clone().unwrap_or_default();
        let right = change.new_text.clone();
        // `oldText: null` (#955's acceptance bar: "renders as a pure
        // addition, not a crash or an empty pane") is deliberately **not**
        // routed through `quadraui::compute_hunks("", right)`: `str::split
        // ('\n')` on `""` yields one empty line, not zero, so diffing
        // against a literal empty string can align that phantom line with
        // a real trailing blank line in `right` and mark it `Same` rather
        // than every row being a clean `Added` — a real, if narrow, gap in
        // treating "no left side at all" as "an empty string" left side.
        // `pure_addition_hunks` builds the addition directly instead.
        let hunks = if is_new_file {
            pure_addition_hunks(&right)
        } else {
            quadraui::compute_hunks(&left, &right)
        };
        let left_label = if is_new_file {
            "(new file)".to_string()
        } else {
            change.path.clone()
        };
        let view = DiffView {
            id: quadraui::WidgetId::new(format!("change-review-{id_suffix}")),
            left,
            right,
            left_label: Some(left_label),
            right_label: Some(change.path.clone()),
            hunks,
            mode: DiffMode::SideBySide,
            editability: DiffEditability::ReadOnly,
            scroll_offset: 0,
            focused_pane: DiffPane::Left,
            has_focus: true,
        };
        Self {
            change,
            view,
            decision: ChangeDecision::Pending,
        }
    }

    /// Row index (into `self.view.flat_rows()`) where each hunk starts, in
    /// order — used to derive "the next/previous hunk" from the current
    /// scroll offset without a separate index field that could drift out
    /// of sync with it.
    fn hunk_start_rows(&self) -> Vec<usize> {
        let mut starts = Vec::with_capacity(self.view.hunks.len());
        let mut acc = 0usize;
        for hunk in &self.view.hunks {
            starts.push(acc);
            acc += hunk.rows.len();
        }
        starts
    }
}

/// The change-review surface's whole state: an ordered list of per-file
/// entries plus which one is currently shown. Source-agnostic by
/// construction — built from a plain `Vec<ProposedChange>`, nothing else.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeReviewState {
    pub entries: Vec<ChangeReviewEntry>,
    pub current: usize,
}

impl ChangeReviewState {
    /// Build a fresh review surface from `changes`. Callers (both #955's
    /// ACP feeder and any future #525 git feeder) should only call this
    /// with a non-empty list — an empty one produces a surface with
    /// nothing to show.
    pub fn new(changes: Vec<ProposedChange>) -> Self {
        let entries = changes
            .into_iter()
            .enumerate()
            .map(|(i, c)| ChangeReviewEntry::new(c, i))
            .collect();
        Self {
            entries,
            current: 0,
        }
    }

    /// Append more entries to an already-open review (a later
    /// `tool_call_update` streaming in another diff, say) rather than
    /// opening a second, competing surface.
    pub fn extend(&mut self, changes: Vec<ProposedChange>) {
        let start = self.entries.len();
        self.entries.extend(
            changes
                .into_iter()
                .enumerate()
                .map(|(i, c)| ChangeReviewEntry::new(c, start + i)),
        );
    }

    pub fn current_entry(&self) -> Option<&ChangeReviewEntry> {
        self.entries.get(self.current)
    }

    pub fn current_entry_mut(&mut self) -> Option<&mut ChangeReviewEntry> {
        self.entries.get_mut(self.current)
    }

    /// Move to the next file, wrapping. No-op on an empty review.
    pub fn next_file(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.current = (self.current + 1) % self.entries.len();
    }

    /// Move to the previous file, wrapping. No-op on an empty review.
    pub fn prev_file(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.current = (self.current + self.entries.len() - 1) % self.entries.len();
    }

    /// Scroll the current entry's `DiffView` to the start of the next
    /// hunk after the current scroll position. No-op past the last hunk.
    pub fn next_hunk(&mut self) {
        let Some(entry) = self.current_entry_mut() else {
            return;
        };
        let starts = entry.hunk_start_rows();
        if let Some(&next) = starts.iter().find(|&&s| s > entry.view.scroll_offset) {
            entry.view.scroll_offset = next;
        }
    }

    /// Scroll the current entry's `DiffView` to the start of the previous
    /// hunk before the current scroll position. No-op before the first
    /// hunk.
    pub fn prev_hunk(&mut self) {
        let Some(entry) = self.current_entry_mut() else {
            return;
        };
        let starts = entry.hunk_start_rows();
        if let Some(&prev) = starts.iter().rev().find(|&&s| s < entry.view.scroll_offset) {
            entry.view.scroll_offset = prev;
        }
    }

    /// Mark the current entry accepted. Returns the accepted change so
    /// the caller (`Engine::change_review_accept_current`) can apply it —
    /// this module stays free of any buffer/filesystem knowledge, keeping
    /// it source- *and* sink-agnostic.
    pub fn accept_current(&mut self) -> Option<ProposedChange> {
        let entry = self.current_entry_mut()?;
        entry.decision = ChangeDecision::Accepted;
        Some(entry.change.clone())
    }

    /// Mark the current entry rejected. No buffer/filesystem effect —
    /// rejecting is purely "don't apply this".
    pub fn reject_current(&mut self) {
        if let Some(entry) = self.current_entry_mut() {
            entry.decision = ChangeDecision::Rejected;
        }
    }

    /// Whether every entry has a terminal decision (accepted or
    /// rejected) — the surface auto-closes once true.
    pub fn all_decided(&self) -> bool {
        !self.entries.is_empty()
            && self
                .entries
                .iter()
                .all(|e| e.decision != ChangeDecision::Pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(path: &str, old: Option<&str>, new: &str) -> ProposedChange {
        ProposedChange {
            path: path.to_string(),
            old_text: old.map(str::to_string),
            new_text: new.to_string(),
        }
    }

    /// The acceptance bar's own words: "a test that feeds it a non-ACP
    /// list" — this constructs `ProposedChange` directly, no
    /// `crate::core::acp` type anywhere in this test, proving the surface
    /// takes a source-agnostic feed.
    #[test]
    fn builds_from_a_hand_rolled_non_acp_change_list() {
        let changes = vec![
            change("src/a.rs", Some("old a\n"), "new a\n"),
            change("src/b.rs", Some("old b\n"), "new b\n"),
        ];
        let state = ChangeReviewState::new(changes);
        assert_eq!(state.entries.len(), 2);
        assert_eq!(state.current, 0);
        assert!(!state.entries[0].view.hunks.is_empty());
    }

    /// `old_text: None` (a new file) renders as a pure addition — every
    /// row's `left` is `None`, not a crash or an empty pane.
    #[test]
    fn new_file_renders_as_pure_addition() {
        let state = ChangeReviewState::new(vec![change("src/new.rs", None, "fn main() {}\n")]);
        let entry = &state.entries[0];
        assert!(!entry.view.hunks.is_empty());
        for row in entry.view.flat_rows() {
            assert!(row.left.is_none(), "new file must have no left-side rows");
        }
    }

    #[test]
    fn next_prev_file_wraps() {
        let mut state =
            ChangeReviewState::new(vec![change("a", Some(""), "a"), change("b", Some(""), "b")]);
        assert_eq!(state.current, 0);
        state.next_file();
        assert_eq!(state.current, 1);
        state.next_file();
        assert_eq!(state.current, 0, "wraps past the last file");
        state.prev_file();
        assert_eq!(state.current, 1, "wraps before the first file");
    }

    #[test]
    fn hunk_navigation_moves_scroll_offset_between_hunks() {
        // Two changes separated by more than twice the diff module's
        // 3-line context radius, so they land in separate hunks instead
        // of merging into one.
        let lines: Vec<String> = (1..=20).map(|n| n.to_string()).collect();
        let old = format!("{}\n", lines.join("\n"));
        let mut changed_lines = lines.clone();
        changed_lines[2] = "CHANGED".to_string();
        changed_lines[15] = "CHANGED".to_string();
        let new = format!("{}\n", changed_lines.join("\n"));
        let mut state = ChangeReviewState::new(vec![change("f", Some(&old), &new)]);
        assert!(
            state.entries[0].view.hunks.len() >= 2,
            "expected at least two hunks from two separated changes"
        );
        let start0 = state.entries[0].view.scroll_offset;
        state.next_hunk();
        let start1 = state.entries[0].view.scroll_offset;
        assert!(start1 > start0, "next_hunk must move scroll forward");
        state.prev_hunk();
        assert_eq!(
            state.entries[0].view.scroll_offset, start0,
            "prev_hunk must return to the first hunk's start"
        );
    }

    #[test]
    fn accept_marks_decision_and_returns_the_change() {
        let mut state = ChangeReviewState::new(vec![change("f", Some("x"), "y")]);
        let accepted = state.accept_current().expect("entry exists");
        assert_eq!(accepted.new_text, "y");
        assert_eq!(state.entries[0].decision, ChangeDecision::Accepted);
    }

    #[test]
    fn reject_marks_decision_without_returning_a_change() {
        let mut state = ChangeReviewState::new(vec![change("f", Some("x"), "y")]);
        state.reject_current();
        assert_eq!(state.entries[0].decision, ChangeDecision::Rejected);
    }

    #[test]
    fn all_decided_true_only_once_every_entry_is_decided() {
        let mut state =
            ChangeReviewState::new(vec![change("a", Some(""), "a"), change("b", Some(""), "b")]);
        assert!(!state.all_decided());
        state.accept_current();
        assert!(!state.all_decided());
        state.next_file();
        state.reject_current();
        assert!(state.all_decided());
    }

    #[test]
    fn extend_appends_without_disturbing_existing_entries() {
        let mut state = ChangeReviewState::new(vec![change("a", Some(""), "a")]);
        state.extend(vec![change("b", Some(""), "b")]);
        assert_eq!(state.entries.len(), 2);
        assert_eq!(state.entries[0].change.path, "a");
        assert_eq!(state.entries[1].change.path, "b");
    }
}
