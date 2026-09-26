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
}
