//! Per-turn change tracking + checkpoints for the ACP change-review surface
//! (#1460), layered on top of #955/#1454's `crate::core::review`: #955's
//! `ChangeReviewState` reviews whatever a `diff` content block *proposes*;
//! this module instead tracks what an agent's turn actually *wrote* (every
//! `fs/write_text_file` served, #954) so the whole turn can be reviewed and
//! reverted after the fact, with a rollback point (`AcpTurnCheckpoint`) per
//! turn that `:AiRestore` can pick.
//!
//! Deliberately free of any `Engine`/buffer/filesystem knowledge, same
//! split as `crate::core::review` / `crate::core::engine::review_ops`:
//! `crate::core::engine::acp_turn_ops` is the only bridge between this pure
//! data model and the rest of the editor.

/// One file's record within a single turn: the content it had the first
/// time the agent wrote it this turn (`pre_turn_content`), and the content
/// of the *last* write the agent made to it this turn
/// (`agent_written_content`). The latter is what [`plan_restore`] compares
/// a file's *current* content against to detect a manual edit made after
/// the agent's own last write — the "must not clobber an unsaved user
/// edit" acceptance bar.
///
/// `pre_turn_content: None` means the path did not exist on disk before
/// the agent's first write this turn (#1460 review non-blocking finding):
/// reverting such an entry must *delete* the file, not write an empty
/// string back — leaving an emptied-but-present file behind would be a
/// surprising outcome for "keep or revert" and doesn't actually undo the
/// agent's "created a new file" action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpTurnFileEntry {
    pub path: String,
    pub pre_turn_content: Option<String>,
    pub agent_written_content: String,
}

/// One completed turn's checkpoint: every file it touched, in first-touch
/// order — the same order the turn's review surface lists them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpTurnCheckpoint {
    pub id: usize,
    pub entries: Vec<AcpTurnFileEntry>,
}

impl AcpTurnCheckpoint {
    /// Record (or update) one write within this checkpoint's turn:
    /// `pre_turn_content` is only ever set the *first* time `path` is
    /// touched (later writes to the same path within the same turn only
    /// move `agent_written_content` forward) — the "captured the first
    /// time each file is touched" acceptance bar.
    pub fn record_write(
        &mut self,
        path: &str,
        pre_content: Option<String>,
        written_content: String,
    ) {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.path == path) {
            entry.agent_written_content = written_content;
        } else {
            self.entries.push(AcpTurnFileEntry {
                path: path.to_string(),
                pre_turn_content: pre_content,
                agent_written_content: written_content,
            });
        }
    }
}

/// One file's planned outcome from [`plan_restore`]: either revert it to
/// its pre-checkpoint content, or refuse because a human edit landed on
/// top of the agent's own last write and would otherwise be clobbered.
///
/// `Revert { pre_turn_content: None, .. }` means the path didn't exist
/// before the earliest relevant checkpoint touched it — reverting it means
/// deleting it, not writing back an empty file (see
/// [`AcpTurnFileEntry`]'s doc for why).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestorePlanItem {
    Revert {
        path: String,
        pre_turn_content: Option<String>,
    },
    Refused {
        path: String,
        reason: String,
    },
}

impl RestorePlanItem {
    pub fn path(&self) -> &str {
        match self {
            RestorePlanItem::Revert { path, .. } => path,
            RestorePlanItem::Refused { path, .. } => path,
        }
    }
}

/// Plan a checkpoint restore across `checkpoints` (oldest first, as
/// [`crate::core::engine::Engine::acp_turn_checkpoints`] always keeps them)
/// back to `target_id` — "revert every later agent write" (#1460's
/// acceptance bar for `:AiRestore`).
///
/// For every path touched by `target_id` or any later checkpoint: plans a
/// revert to the content it had *before the earliest of those turns first
/// touched it*, unless `current_content(path)` (a caller looks this up
/// buffer-first, since this module has no filesystem/buffer access of its
/// own) no longer matches the *last* content any relevant checkpoint
/// recorded the agent writing — meaning a human edit landed on the file
/// since, which must be refused rather than clobbered.
pub fn plan_restore(
    checkpoints: &[AcpTurnCheckpoint],
    target_id: usize,
    current_content: impl Fn(&str) -> Option<String>,
) -> Vec<RestorePlanItem> {
    let mut restore_to: std::collections::BTreeMap<String, Option<String>> =
        std::collections::BTreeMap::new();
    let mut last_written: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for cp in checkpoints.iter().filter(|c| c.id >= target_id) {
        for entry in &cp.entries {
            if !restore_to.contains_key(&entry.path) {
                restore_to.insert(entry.path.clone(), entry.pre_turn_content.clone());
                order.push(entry.path.clone());
            }
            last_written.insert(entry.path.clone(), entry.agent_written_content.clone());
        }
    }
    order
        .into_iter()
        .map(|path| {
            let expected = last_written.get(&path).cloned().unwrap_or_default();
            let current = current_content(&path).unwrap_or_default();
            if current == expected {
                let pre_turn_content = restore_to.get(&path).cloned().flatten();
                RestorePlanItem::Revert {
                    path,
                    pre_turn_content,
                }
            } else {
                RestorePlanItem::Refused {
                    path,
                    reason: "file has changed since the agent's last write \u{2014} refusing to \
                             overwrite an edit made in between"
                        .to_string(),
                }
            }
        })
        .collect()
}

/// Per-line change classification for a turn-diff gutter overlay
/// (`badge`/`off` mode, #1515) — `old` is a checkpoint entry's
/// `pre_turn_content` (`None` meaning the agent created the path, so every
/// line paints [`crate::core::git::GitLineStatus::Added`]), `new` is the
/// path's *current* content. Line 0 of the result corresponds to line 0
/// of `new` split on `'\n'` — the same convention `ropey::Rope::len_lines`
/// uses (a trailing `'\n'` yields one trailing empty line, matching
/// `quadraui::compute_hunks`'s own `split('\n')`), so a caller indexing
/// with a buffer's own `line_idx` needs no adjustment.
///
/// Deliberately independent of [`crate::core::git::compute_file_diff`] —
/// that diffs the working copy against `git diff HEAD`, the wrong base
/// here: a file can be turn-dirty without being git-dirty (already
/// committed once this turn started) and vice versa (already git-dirty
/// *before* the turn touched it), so the two overlays must be able to
/// disagree.
pub fn line_status(old: Option<&str>, new: &str) -> Vec<Option<crate::core::git::GitLineStatus>> {
    use crate::core::git::GitLineStatus;
    let total = new.split('\n').count();
    let Some(old) = old else {
        return vec![Some(GitLineStatus::Added); total];
    };
    let mut result = vec![None; total];
    for hunk in quadraui::compute_hunks(old, new) {
        let mut right_idx = hunk.right_start.saturating_sub(1);
        let mut pending_removed = false;
        for row in &hunk.rows {
            // A pure-deletion row has no right-side line of its own; once
            // the run of consecutive `Removed` rows ends, flush a marker
            // anchored at the boundary those deletions sit at *right now*
            // — the same convention `crate::core::git::parse_unified_diff`
            // uses for a real `git diff` hunk's pure deletions. Flushing
            // per run (not once per hunk) is what lets two separate
            // deletion runs within a single hunk each get their own
            // marker instead of collapsing into (at most) one.
            if row.kind != quadraui::DiffRowKind::Removed && pending_removed {
                let mark = right_idx.min(total.saturating_sub(1));
                if result.get(mark).is_some_and(|v| v.is_none()) {
                    result[mark] = Some(GitLineStatus::Deleted);
                }
                pending_removed = false;
            }
            match row.kind {
                quadraui::DiffRowKind::Same => right_idx += 1,
                quadraui::DiffRowKind::Added => {
                    if right_idx < total {
                        result[right_idx] = Some(GitLineStatus::Added);
                    }
                    right_idx += 1;
                }
                quadraui::DiffRowKind::Changed => {
                    if right_idx < total {
                        result[right_idx] = Some(GitLineStatus::Modified);
                    }
                    right_idx += 1;
                }
                quadraui::DiffRowKind::Removed => pending_removed = true,
            }
        }
        // Trailing pure-deletions at the end of the hunk (mirrors
        // `parse_unified_diff`'s own end-of-diff flush).
        if pending_removed {
            let mark = right_idx.min(total.saturating_sub(1));
            if result.get(mark).is_some_and(|v| v.is_none()) {
                result[mark] = Some(GitLineStatus::Deleted);
            }
        }
    }
    result
}

/// `(added, removed)` line counts for the same diff [`line_status`]
/// classifies — the `badge` mode's "Edited N files · +a -r" status-strip
/// summary (`Engine::acp_turn_review_badge`). A `Changed` row (one line
/// replaced by another) counts as one added *and* one removed line,
/// matching `git diff --stat`'s own convention.
pub fn count_changed_lines(old: Option<&str>, new: &str) -> (usize, usize) {
    let Some(old) = old else {
        return (new.split('\n').count(), 0);
    };
    let mut added = 0usize;
    let mut removed = 0usize;
    for hunk in quadraui::compute_hunks(old, new) {
        for row in &hunk.rows {
            match row.kind {
                quadraui::DiffRowKind::Added => added += 1,
                quadraui::DiffRowKind::Removed => removed += 1,
                quadraui::DiffRowKind::Changed => {
                    added += 1;
                    removed += 1;
                }
                quadraui::DiffRowKind::Same => {}
            }
        }
    }
    (added, removed)
}

/// Hunk-granularity version of [`plan_restore`]'s whole-file "edited since"
/// check (#1516): whether the hunk identified by `left_start`/`rows` (as
/// painted from the *current* diff between a turn entry's
/// `pre_turn_content` and its live content) still matches exactly what the
/// agent's own last write (`agent_written`) produced there. `left_start` —
/// a position within the immutable pre-turn text — is used rather than the
/// hunk's own (shiftable) right-side position, the same stable-identity
/// trick [`crate::core::review::ChangeReviewEntry::refresh_after_edit`]
/// uses to carry decisions across a recompute.
///
/// `false` ("edited by you") either because the agent's own diff has no
/// hunk at that same pre-turn position at all (a human introduced a wholly
/// new change there after the agent finished), or because it does but its
/// right-side text differs from `rows`'.
///
/// `pre_turn: None` (the agent created this path) reduces to plain
/// whole-file equality against `agent_written` rather than re-deriving
/// hunks against a literal `""` left side — the same phantom-empty-line
/// hazard `crate::core::review`'s `pure_addition_hunks` doc warns a diff
/// against a bare `""` has, and moot anyway since a brand-new path only
/// ever has the one hunk covering its entire content.
pub fn hunk_matches_agents_last_write(
    pre_turn: Option<&str>,
    agent_written: &str,
    left_start: usize,
    rows: &[quadraui::DiffRow],
) -> bool {
    let now_right: Vec<&str> = rows.iter().filter_map(|r| r.right.as_deref()).collect();
    let Some(left_text) = pre_turn else {
        let now_joined = now_right.join("\n");
        return now_joined.trim_end_matches('\n') == agent_written.trim_end_matches('\n');
    };
    let agent_hunks = quadraui::compute_hunks(left_text, agent_written);
    let Some(agent_hunk) = agent_hunks.iter().find(|h| h.left_start == left_start) else {
        return false;
    };
    let agent_right: Vec<&str> = agent_hunk
        .rows
        .iter()
        .filter_map(|r| r.right.as_deref())
        .collect();
    now_right == agent_right
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, pre: &str, written: &str) -> AcpTurnFileEntry {
        AcpTurnFileEntry {
            path: path.to_string(),
            pre_turn_content: Some(pre.to_string()),
            agent_written_content: written.to_string(),
        }
    }

    #[test]
    fn record_write_captures_pre_content_only_on_first_touch() {
        let mut cp = AcpTurnCheckpoint {
            id: 1,
            entries: vec![],
        };
        cp.record_write("f.rs", Some("orig".to_string()), "v1".to_string());
        cp.record_write(
            "f.rs",
            Some("should-be-ignored".to_string()),
            "v2".to_string(),
        );
        assert_eq!(
            cp.entries.len(),
            1,
            "same-turn re-write must not duplicate the entry"
        );
        assert_eq!(cp.entries[0].pre_turn_content, Some("orig".to_string()));
        assert_eq!(cp.entries[0].agent_written_content, "v2");
    }

    /// #1460 review (non-blocking finding): a path that did not exist
    /// before the agent's first write this turn must record `None`, not an
    /// empty string — the two are indistinguishable to a naive `String`
    /// "restore" (both mean "write back empty"), but only `None` correctly
    /// tells [`plan_restore`] to plan a *delete* rather than an empty
    /// overwrite.
    #[test]
    fn record_write_with_no_pre_content_records_none_for_a_brand_new_path() {
        let mut cp = AcpTurnCheckpoint {
            id: 1,
            entries: vec![],
        };
        cp.record_write("new.rs", None, "created by agent".to_string());
        assert_eq!(cp.entries[0].pre_turn_content, None);
    }

    #[test]
    fn plan_restore_reverts_every_file_the_target_checkpoint_touched() {
        let checkpoints = vec![AcpTurnCheckpoint {
            id: 1,
            entries: vec![
                entry("a.rs", "orig-a", "written-a"),
                entry("b.rs", "orig-b", "written-b"),
            ],
        }];
        let plan = plan_restore(&checkpoints, 1, |path| match path {
            "a.rs" => Some("written-a".to_string()),
            "b.rs" => Some("written-b".to_string()),
            _ => None,
        });
        assert_eq!(plan.len(), 2);
        assert_eq!(
            plan[0],
            RestorePlanItem::Revert {
                path: "a.rs".to_string(),
                pre_turn_content: Some("orig-a".to_string()),
            }
        );
        assert_eq!(
            plan[1],
            RestorePlanItem::Revert {
                path: "b.rs".to_string(),
                pre_turn_content: Some("orig-b".to_string()),
            }
        );
    }

    /// The acceptance bar's own words: a file whose current content no
    /// longer matches what the agent last wrote (a human edited it since)
    /// must be refused, not clobbered.
    #[test]
    fn plan_restore_refuses_a_file_edited_by_a_human_since_the_agents_last_write() {
        let checkpoints = vec![AcpTurnCheckpoint {
            id: 1,
            entries: vec![entry("a.rs", "orig-a", "written-a")],
        }];
        let plan = plan_restore(&checkpoints, 1, |_| Some("human edit".to_string()));
        assert_eq!(
            plan,
            vec![RestorePlanItem::Refused {
                path: "a.rs".to_string(),
                reason: "file has changed since the agent's last write \u{2014} refusing to \
                         overwrite an edit made in between"
                    .to_string(),
            }]
        );
    }

    /// Restoring an *earlier* checkpoint than the most recent one must
    /// undo every later turn's writes too, using each path's *earliest*
    /// relevant pre-turn content — "reverts every later agent write", not
    /// just the target turn's own.
    #[test]
    fn plan_restore_undoes_every_later_checkpoint_too() {
        let checkpoints = vec![
            AcpTurnCheckpoint {
                id: 1,
                entries: vec![entry("a.rs", "orig-a", "turn1-a")],
            },
            AcpTurnCheckpoint {
                id: 2,
                entries: vec![entry("a.rs", "turn1-a", "turn2-a")],
            },
        ];
        let plan = plan_restore(&checkpoints, 1, |_| Some("turn2-a".to_string()));
        assert_eq!(
            plan,
            vec![RestorePlanItem::Revert {
                path: "a.rs".to_string(),
                pre_turn_content: Some("orig-a".to_string()),
            }],
            "restoring turn 1 must go all the way back to before turn 1 touched it, \
             not just undo turn 2"
        );
    }

    /// A checkpoint older than `target_id` must not be touched at all —
    /// only `target_id` and later are in scope.
    #[test]
    fn plan_restore_ignores_checkpoints_before_the_target() {
        let checkpoints = vec![
            AcpTurnCheckpoint {
                id: 1,
                entries: vec![entry("a.rs", "orig-a", "turn1-a")],
            },
            AcpTurnCheckpoint {
                id: 2,
                entries: vec![entry("b.rs", "orig-b", "turn2-b")],
            },
        ];
        let plan = plan_restore(&checkpoints, 2, |_| Some("turn2-b".to_string()));
        assert_eq!(
            plan,
            vec![RestorePlanItem::Revert {
                path: "b.rs".to_string(),
                pre_turn_content: Some("orig-b".to_string()),
            }]
        );
    }

    /// #1460 review (non-blocking finding): a file the agent *created*
    /// (no pre-turn content on disk) must plan a `None` restore — a
    /// delete — not `Some("")` — an empty-but-present file.
    #[test]
    fn plan_restore_plans_a_delete_for_a_file_the_agent_created() {
        let checkpoints = vec![AcpTurnCheckpoint {
            id: 1,
            entries: vec![AcpTurnFileEntry {
                path: "new.rs".to_string(),
                pre_turn_content: None,
                agent_written_content: "created by agent".to_string(),
            }],
        }];
        let plan = plan_restore(&checkpoints, 1, |_| Some("created by agent".to_string()));
        assert_eq!(
            plan,
            vec![RestorePlanItem::Revert {
                path: "new.rs".to_string(),
                pre_turn_content: None,
            }],
            "reverting an agent-created file must plan a delete (None), not an empty overwrite"
        );
    }

    /// Review finding (#1515 fix iteration 1): two separate deletion runs
    /// within a single diff hunk must each get their own `Deleted` marker
    /// — not collapse into (at most) one at the position of the *last*
    /// run, silently dropping the earlier one. `old` has 10 lines, `new`
    /// deletes both "2" (right after line "1") and "9" (right before line
    /// "10") — both within the same hunk since the two deletions are
    /// close enough that quadraui's default context window merges them.
    #[test]
    fn line_status_marks_every_separate_deletion_run_in_a_single_hunk() {
        use crate::core::git::GitLineStatus;
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10";
        let new = "1\n3\n4\n5\n6\n7\n8\n10";
        let hunks = quadraui::compute_hunks(old, new);
        assert_eq!(
            hunks.len(),
            1,
            "test setup: expected both deletions to land in a single hunk"
        );

        let statuses = line_status(Some(old), new);
        // new's lines: 0:"1" 1:"3" 2:"4" 3:"5" 4:"6" 5:"7" 6:"8" 7:"10"
        // Same convention `parse_unified_diff` uses (see
        // `test_parse_unified_diff_pure_deletion`): a pure-deletion run is
        // anchored at the first surviving line *after* it, not before.
        assert_eq!(
            statuses[1],
            Some(GitLineStatus::Deleted),
            "the deletion of \"2\" must be anchored at \"3\" (index 1), the \
             first surviving line after it — not lost or merged into the \
             later deletion's marker"
        );
        assert_eq!(
            statuses[7],
            Some(GitLineStatus::Deleted),
            "the deletion of \"9\" must be anchored at \"10\" (index 7), the \
             first surviving line after it"
        );
        for (idx, status) in statuses.iter().enumerate() {
            if idx != 1 && idx != 7 {
                assert_eq!(
                    *status, None,
                    "unchanged line at index {idx} must not be marked"
                );
            }
        }
    }

    #[test]
    fn line_status_marks_a_single_in_place_edit_as_modified() {
        use crate::core::git::GitLineStatus;
        let old = "a\nb\nc";
        let new = "a\nB\nc";
        assert_eq!(
            line_status(Some(old), new),
            vec![None, Some(GitLineStatus::Modified), None]
        );
    }

    #[test]
    fn line_status_marks_every_line_added_when_no_pre_turn_content() {
        use crate::core::git::GitLineStatus;
        let new = "a\nb";
        assert_eq!(
            line_status(None, new),
            vec![Some(GitLineStatus::Added), Some(GitLineStatus::Added)]
        );
    }

    // ── hunk_matches_agents_last_write (#1516) ──────────────────────────

    fn only_hunk(pre_turn: &str, now: &str) -> (usize, Vec<quadraui::DiffRow>) {
        let hunk = quadraui::compute_hunks(pre_turn, now)
            .into_iter()
            .next()
            .expect("test setup: a real diff");
        (hunk.left_start, hunk.rows)
    }

    /// The common case: nothing touched the file since the agent's last
    /// write — the hunk the review is currently painting is *exactly* the
    /// agent's own hunk, so this must read as "still matches", not "edited
    /// by you".
    #[test]
    fn hunk_matches_when_now_equals_the_agents_last_write() {
        let pre = "1\n2\n3\n";
        let agent_written = "1\nAGENT\n3\n";
        let (left_start, rows) = only_hunk(pre, agent_written);
        assert!(hunk_matches_agents_last_write(
            Some(pre),
            agent_written,
            left_start,
            &rows
        ));
    }

    /// The exact scenario #1516 names: a human hand-edits the same region
    /// the agent last wrote, after the fact — the hunk's own right-side
    /// text no longer agrees with what the agent produced there.
    #[test]
    fn hunk_does_not_match_after_a_human_edit_on_top_of_the_agents_hunk() {
        let pre = "1\n2\n3\n";
        let agent_written = "1\nAGENT\n3\n";
        let human_now = "1\nHUMAN EDIT\n3\n";
        let (left_start, rows) = only_hunk(pre, human_now);
        assert!(!hunk_matches_agents_last_write(
            Some(pre),
            agent_written,
            left_start,
            &rows
        ));
    }

    /// A human edit *elsewhere* in the file (a region the agent's own diff
    /// never touched, so it never appears in `agent_written`'s hunks at
    /// all) — one of the "no matching hunk at all" cases, also "edited by
    /// you" for that hunk specifically.
    #[test]
    fn hunk_does_not_match_a_brand_new_human_only_change() {
        let pre = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        // The agent never touched this file at all this turn (a
        // degenerate but legal input: "agent_written" identical to `pre`).
        let agent_written = pre;
        let human_now = "1\n2\n3\n4\nHUMAN\n6\n7\n8\n9\n10\n";
        let (left_start, rows) = only_hunk(pre, human_now);
        assert!(!hunk_matches_agents_last_write(
            Some(pre),
            agent_written,
            left_start,
            &rows
        ));
    }

    /// `pre_turn: None` (agent-created path) reduces to whole-file equality
    /// against `agent_written` — matches when nothing has changed since.
    #[test]
    fn hunk_matches_for_a_new_file_when_untouched_since_the_agents_write() {
        let agent_written = "created by agent\n";
        let now = agent_written;
        let (left_start, rows) = only_hunk("", now);
        // `left_start` from a `("", now)` diff isn't meaningful for the
        // `None` branch (it never consults it) — passed through as-is.
        assert!(hunk_matches_agents_last_write(
            None,
            agent_written,
            left_start,
            &rows
        ));
    }
}
