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

/// A single pinned review comment (#527, Track A Phase 3): a short note
/// anchored to one file/line within an open [`ChangeReviewState`],
/// collected into [`ChangeReviewState::comments`] and folded into the
/// verdict body #526 composes (via [`markdown_findings_serializer`], or
/// whichever [`FindingsSerializer`] the host has installed). `file` is
/// whatever path the reviewed [`ProposedChange`] itself carries (the same
/// string [`ChangeReviewEntry::change`]'s `path` uses — not necessarily
/// absolute), `line` is 1-based, matching every other line number this
/// module already hands out (`row_to_location`, `DiffHunk::left_start`/
/// `right_start`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    pub file: String,
    pub line: u32,
    pub text: String,
}

/// A verdict on the review as a whole, reported through a provider-declared
/// command (#526) — distinct from [`ChangeDecision`], which is per-file
/// accept/reject within the diff surface itself. Generic vocabulary: these
/// are universal code-review terms (GitHub, GitLab, Gerrit all use them),
/// not any specific pipeline tool's — `src/core/` and `src/render.rs` must
/// name no coordinator-specific verdict wording (checked by a dedicated
/// repo-root test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewVerdict {
    Approve,
    RequestChanges,
    Comment,
}

impl ReviewVerdict {
    /// The stable string this verdict is keyed by — both the key a
    /// provider's manifest uses in
    /// `crate::core::extensions::BoardProviderConfig::verdict_commands`,
    /// and the token substituted for the literal `{verdict}` in a resolved
    /// argv (`BoardProviderConfig::verdict_argv`), matching the `{id}`/
    /// `{body_file}` substitution convention the same method uses.
    pub fn token(self) -> &'static str {
        match self {
            ReviewVerdict::Approve => "approve",
            ReviewVerdict::RequestChanges => "request-changes",
            ReviewVerdict::Comment => "comment",
        }
    }
}

/// A findings-list serializer (#527): turns the pinned [`ReviewComment`]s
/// collected on a [`ChangeReviewState`] into the text a verdict composer
/// buffer is prefilled with. Deliberately just a plain function pointer,
/// not a hardcoded call — the issue's own acceptance bar is "the findings
/// serializer is pluggable, not hardcoded to one provider's format", so
/// `crate::core::engine::Engine::review_findings_serializer` holds one of
/// these (defaulting to [`markdown_findings_serializer`]) and calls
/// through it rather than formatting a string inline; a host that wants a
/// provider-specific findings layout swaps the field (`Engine::
/// set_review_findings_serializer` in tests today; a real per-provider
/// hook is future work, but nothing here would need to change shape to
/// add one — see this module's own tests for two different serializers
/// producing two different bodies from the same `Vec<ReviewComment>`).
pub type FindingsSerializer = fn(&[ReviewComment]) -> String;

/// The generic default [`FindingsSerializer`]: a Markdown bullet list,
/// `- **file:line** — text` per finding, under a `## Findings` heading —
/// the same universal, no-specific-tool's-wire-format bar
/// [`ReviewVerdict::token`] already holds itself to (every mainstream
/// review UI renders a Markdown bullet list the same way). Comments are
/// listed in collection order (the order they were pinned in), not
/// sorted by file/line — [`ChangeReviewState::comments`] is already a
/// plain append/edit/delete list, so preserving that order is "no
/// surprise reordering" for the human proof-reading the composed body
/// before `:w`.
///
/// Empty `comments` produces an empty string, so a review with no pinned
/// findings composes exactly the blank body it did before #527.
pub fn markdown_findings_serializer(comments: &[ReviewComment]) -> String {
    if comments.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Findings\n\n");
    for c in comments {
        out.push_str(&format!("- **{}:{}** — {}\n", c.file, c.line, c.text));
    }
    out.push('\n');
    out
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
    /// Pinned line comments collected while this review is open (#527,
    /// Track A Phase 3) — provider-agnostic, in pin order. Never sorted or
    /// deduplicated automatically; [`Self::comment_index_at`] is how a
    /// caller finds "the comment already on this line" to edit/delete
    /// rather than accumulating duplicates.
    pub comments: Vec<ReviewComment>,
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
            comments: Vec::new(),
        }
    }

    /// Append more entries to an already-open review (a later
    /// `tool_call_update` streaming in another diff, say) rather than
    /// opening a second, competing surface.
    ///
    /// Skips any incoming `change` that is byte-identical (`path`,
    /// `old_text`, `new_text` all equal) to an entry already present —
    /// a guard against a replaying/buggy ACP agent that re-announces the
    /// same `toolCallId` with the same `diff` content producing a second,
    /// redundant entry for the same edit. This module stays source-agnostic
    /// (no `toolCallId` in [`ProposedChange`]), so "identical" is the only
    /// signal available here; a change to the same `path` with different
    /// text is a real edit and is still appended, matching the "addressable
    /// collection" intent one level up (`Engine::acp_upsert_tool_call`
    /// upserts by id; this is the same policy applied to content it
    /// can't key by id).
    pub fn extend(&mut self, changes: Vec<ProposedChange>) {
        let mut next_id = self.entries.len();
        for change in changes {
            if self.entries.iter().any(|e| e.change == change) {
                continue;
            }
            self.entries.push(ChangeReviewEntry::new(change, next_id));
            next_id += 1;
        }
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

    /// `(path, 1-based line)` the current entry's `DiffView` is scrolled
    /// to — the same row `Return`'s jump-to-hit resolves, reused here so
    /// "comment on the current line" always agrees with "jump to the
    /// current line" about which line that is. `None` on an empty review
    /// or a scroll offset that has drifted past every hunk (shouldn't
    /// happen in practice, but `row_to_location` is a plain lookup that
    /// can fail, so this stays fallible rather than panicking).
    pub fn current_location(&self) -> Option<(String, u32)> {
        let entry = self.current_entry()?;
        row_to_location(entry, entry.view.scroll_offset)
    }

    /// Index into [`Self::comments`] of the comment already pinned to
    /// `file`/`line`, if any — how a caller distinguishes "add a new
    /// comment here" from "edit the one that's already here".
    pub fn comment_index_at(&self, file: &str, line: u32) -> Option<usize> {
        self.comments
            .iter()
            .position(|c| c.file == file && c.line == line)
    }

    /// Overwrite `comments[index]`'s text in place. No-op (returns
    /// `false`) if `index` is out of range — a stale index from a dialog
    /// that outlived a comment someone else deleted in the meantime,
    /// say.
    pub fn edit_comment(&mut self, index: usize, text: String) -> bool {
        match self.comments.get_mut(index) {
            Some(c) => {
                c.text = text;
                true
            }
            None => false,
        }
    }

    /// Remove and return `comments[index]`, if it exists.
    pub fn delete_comment(&mut self, index: usize) -> Option<ReviewComment> {
        if index < self.comments.len() {
            Some(self.comments.remove(index))
        } else {
            None
        }
    }
}

/// Derive a 1-based `(path, line)` for `row_idx` (an index into
/// `entry.view.flat_rows()`) from the hunk that contains it — the same
/// per-row arithmetic `quadraui::unified_hunk_header` uses per-hunk, just
/// walked row by row. Prefers the right-side (new) line number, falling
/// back to the left-side one for a pure-removal row that has no right
/// side at all.
///
/// `pub(crate)` rather than private: both `Engine::change_review_jump_to_hit`
/// (`crate::core::engine::review_ops`) and [`ChangeReviewState::
/// current_location`] above need the exact same row → location mapping —
/// keeping it here as the single definition is what makes "comment on the
/// current line" and "jump to the current line" structurally unable to
/// disagree about which line that is.
pub(crate) fn row_to_location(entry: &ChangeReviewEntry, row_idx: usize) -> Option<(String, u32)> {
    let mut acc = 0usize;
    for hunk in &entry.view.hunks {
        if row_idx < acc + hunk.rows.len() {
            let offset = row_idx - acc;
            let mut left_line = hunk.left_start as u32;
            let mut right_line = hunk.right_start as u32;
            for row in &hunk.rows[..offset] {
                if row.left.is_some() {
                    left_line += 1;
                }
                if row.right.is_some() {
                    right_line += 1;
                }
            }
            let row = &hunk.rows[offset];
            let line = if row.right.is_some() {
                right_line
            } else {
                left_line
            };
            return Some((entry.change.path.clone(), line));
        }
        acc += hunk.rows.len();
    }
    None
}

/// Build a copy of `entry.view` with any [`ReviewComment`]s pinned to
/// `entry.change.path` appended inline to their row's rendered text — the
/// "existing line-annotation / virtual-text channel" #527 names
/// (`src/core/engine/plugins.rs`'s `ctx.annotate_lines` -> `RenderedLine::
/// annotation`) reused for the diff surface: extra text spliced into what
/// gets *painted*, never touching the underlying [`ProposedChange`]/diff
/// data itself, same "virtual, not real" contract that channel already
/// holds for the plain editor buffer. `quadraui::DiffRow` has no
/// dedicated annotation slot (unlike `RenderedLine`), so the text is
/// appended directly onto the row's `right` (or `left`, for a pure-removal
/// row with no right side) string — since `draw_diff_view` takes ordinary
/// row text, this is the whole mechanism, no backend-specific code on
/// either side needed.
///
/// Returns a borrowed `Cow` when there is nothing to add (no comments at
/// all, or none on this file) — the common case, so a review with no
/// pinned findings costs no per-frame clone.
pub fn view_with_inline_comments<'a>(
    comments: &[ReviewComment],
    entry: &'a ChangeReviewEntry,
) -> std::borrow::Cow<'a, DiffView> {
    let relevant: Vec<&ReviewComment> = comments
        .iter()
        .filter(|c| c.file == entry.change.path)
        .collect();
    if relevant.is_empty() {
        return std::borrow::Cow::Borrowed(&entry.view);
    }
    let mut view = entry.view.clone();
    for hunk in &mut view.hunks {
        let mut left_line = hunk.left_start as u32;
        let mut right_line = hunk.right_start as u32;
        for row in &mut hunk.rows {
            let line = if row.right.is_some() {
                right_line
            } else {
                left_line
            };
            let on_this_row: Vec<&str> = relevant
                .iter()
                .filter(|c| c.line == line)
                .map(|c| c.text.as_str())
                .collect();
            if !on_this_row.is_empty() {
                let suffix = format!("   [comment] {}", on_this_row.join(" | "));
                if let Some(right) = &mut row.right {
                    right.push_str(&suffix);
                } else if let Some(left) = &mut row.left {
                    left.push_str(&suffix);
                }
            }
            if row.left.is_some() {
                left_line += 1;
            }
            if row.right.is_some() {
                right_line += 1;
            }
        }
    }
    std::borrow::Cow::Owned(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_verdict_tokens_are_stable_and_distinct() {
        assert_eq!(ReviewVerdict::Approve.token(), "approve");
        assert_eq!(ReviewVerdict::RequestChanges.token(), "request-changes");
        assert_eq!(ReviewVerdict::Comment.token(), "comment");
    }

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

    /// Guard against a replaying/buggy ACP agent re-announcing the same
    /// `toolCallId` with the same `diff` content: `extend` must not
    /// produce a second, byte-identical entry for the same edit.
    #[test]
    fn extend_skips_a_byte_identical_duplicate_change() {
        let mut state = ChangeReviewState::new(vec![change("a", Some("x"), "y")]);
        state.extend(vec![change("a", Some("x"), "y")]);
        assert_eq!(
            state.entries.len(),
            1,
            "re-announcing an identical change must not duplicate the entry"
        );
    }

    /// A same-path change with different text IS a real edit (e.g. the
    /// agent revising its own proposal) and must still be appended, not
    /// swallowed by the duplicate guard above.
    #[test]
    fn extend_still_appends_a_same_path_change_with_different_text() {
        let mut state = ChangeReviewState::new(vec![change("a", Some("x"), "y")]);
        state.extend(vec![change("a", Some("x"), "z")]);
        assert_eq!(state.entries.len(), 2);
        assert_eq!(state.entries[1].change.new_text, "z");
    }

    // ── #527 Track A Phase 3: pinned line comments ────────────────────

    #[test]
    fn new_review_starts_with_no_comments() {
        let state = ChangeReviewState::new(vec![change("f", Some("a"), "b")]);
        assert!(state.comments.is_empty());
    }

    /// `current_location` must resolve to the same `(path, line)` that
    /// `row_to_location(entry, scroll_offset)` would — this is the
    /// contract "comment on the current line" depends on for agreeing
    /// with "jump to the current line" about which line that is.
    #[test]
    fn current_location_resolves_the_scrolled_to_row() {
        let state = ChangeReviewState::new(vec![change("f.rs", Some("old\n"), "new\n")]);
        let (path, line) = state.current_location().expect("a fresh review has a row");
        assert_eq!(path, "f.rs");
        assert_eq!(line, 1);
    }

    #[test]
    fn current_location_is_none_on_an_empty_review() {
        let state = ChangeReviewState {
            entries: vec![],
            current: 0,
            comments: vec![],
        };
        assert!(state.current_location().is_none());
    }

    #[test]
    fn comment_index_at_finds_an_existing_pin_and_nothing_else() {
        let mut state = ChangeReviewState::new(vec![change("f", Some("a"), "b")]);
        state.comments.push(ReviewComment {
            file: "f".to_string(),
            line: 3,
            text: "nit".to_string(),
        });
        assert_eq!(state.comment_index_at("f", 3), Some(0));
        assert_eq!(state.comment_index_at("f", 4), None);
        assert_eq!(state.comment_index_at("other", 3), None);
    }

    #[test]
    fn edit_comment_overwrites_text_in_place() {
        let mut state = ChangeReviewState::new(vec![change("f", Some("a"), "b")]);
        state.comments.push(ReviewComment {
            file: "f".to_string(),
            line: 1,
            text: "before".to_string(),
        });
        assert!(state.edit_comment(0, "after".to_string()));
        assert_eq!(state.comments[0].text, "after");
        assert!(
            !state.edit_comment(5, "nope".to_string()),
            "an out-of-range index must not panic or silently succeed"
        );
    }

    #[test]
    fn delete_comment_removes_and_returns_it() {
        let mut state = ChangeReviewState::new(vec![change("f", Some("a"), "b")]);
        state.comments.push(ReviewComment {
            file: "f".to_string(),
            line: 1,
            text: "gone soon".to_string(),
        });
        let removed = state.delete_comment(0).expect("index 0 exists");
        assert_eq!(removed.text, "gone soon");
        assert!(state.comments.is_empty());
        assert!(state.delete_comment(0).is_none(), "already empty");
    }

    /// #527 acceptance: "render pinned comments inline". Asserted on the
    /// rendered `DiffView` text a backend would actually paint, not on
    /// `comments` being populated — a comment pinned to a row must show
    /// up spliced into that exact row's content, not some other row's.
    #[test]
    fn view_with_inline_comments_splices_text_into_the_right_row() {
        let state = ChangeReviewState::new(vec![change("f.rs", Some("a\nb\nc\n"), "a\nB\nc\n")]);
        let entry = &state.entries[0];
        let comments = vec![ReviewComment {
            file: "f.rs".to_string(),
            line: 2,
            text: "why change this?".to_string(),
        }];
        let view = view_with_inline_comments(&comments, entry);
        let row = view
            .flat_rows()
            .into_iter()
            .find(|r| r.right.as_deref() == Some("B") || r.left.as_deref() == Some("b"))
            .expect("the changed line 2 must still be present");
        let painted = row.right.as_deref().or(row.left.as_deref()).unwrap();
        assert!(
            painted.contains("why change this?"),
            "the comment text must be spliced into line 2's own row, got: {painted:?}"
        );
        // A different, uncommented row must be untouched.
        let untouched = view
            .flat_rows()
            .into_iter()
            .find(|r| r.right.as_deref() == Some("a") || r.left.as_deref() == Some("a"))
            .expect("line 1 must still be present");
        let untouched_text = untouched
            .right
            .as_deref()
            .or(untouched.left.as_deref())
            .unwrap();
        assert!(!untouched_text.contains("why change this?"));
    }

    #[test]
    fn view_with_inline_comments_is_borrowed_when_there_is_nothing_to_add() {
        let state = ChangeReviewState::new(vec![change("f.rs", Some("a\n"), "b\n")]);
        let entry = &state.entries[0];
        let view = view_with_inline_comments(&[], entry);
        assert!(matches!(view, std::borrow::Cow::Borrowed(_)));

        // A comment on a *different* file must not force a clone either.
        let other_file_comment = vec![ReviewComment {
            file: "other.rs".to_string(),
            line: 1,
            text: "irrelevant here".to_string(),
        }];
        let view = view_with_inline_comments(&other_file_comment, entry);
        assert!(matches!(view, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn markdown_findings_serializer_is_empty_for_no_comments() {
        assert_eq!(markdown_findings_serializer(&[]), "");
    }

    #[test]
    fn markdown_findings_serializer_lists_file_line_and_text_in_order() {
        let comments = vec![
            ReviewComment {
                file: "a.rs".to_string(),
                line: 3,
                text: "first".to_string(),
            },
            ReviewComment {
                file: "b.rs".to_string(),
                line: 10,
                text: "second".to_string(),
            },
        ];
        let body = markdown_findings_serializer(&comments);
        let first_pos = body.find("a.rs:3").expect("first finding must appear");
        let second_pos = body.find("b.rs:10").expect("second finding must appear");
        assert!(
            first_pos < second_pos,
            "findings must serialize in pin order, not sorted"
        );
        assert!(body.contains("first"));
        assert!(body.contains("second"));
    }

    /// #527's own acceptance bar: "the findings serializer is pluggable,
    /// not hardcoded to one provider's format" — proven here by two
    /// different [`FindingsSerializer`]s producing two different bodies
    /// from the identical `Vec<ReviewComment>`, both reachable through the
    /// same `FindingsSerializer` function-pointer type.
    #[test]
    fn findings_serializer_is_pluggable_not_a_single_hardcoded_format() {
        fn plain_csv(comments: &[ReviewComment]) -> String {
            comments
                .iter()
                .map(|c| format!("{},{},{}", c.file, c.line, c.text))
                .collect::<Vec<_>>()
                .join("\n")
        }
        let comments = vec![ReviewComment {
            file: "x.rs".to_string(),
            line: 1,
            text: "note".to_string(),
        }];
        let via_default: FindingsSerializer = markdown_findings_serializer;
        let via_custom: FindingsSerializer = plain_csv;
        let default_out = via_default(&comments);
        let custom_out = via_custom(&comments);
        assert_ne!(default_out, custom_out);
        assert!(default_out.contains("**x.rs:1**"));
        assert_eq!(custom_out, "x.rs,1,note");
    }
}
