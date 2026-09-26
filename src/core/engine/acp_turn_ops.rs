//! `Engine` wiring for per-turn ACP change tracking + checkpoints (#1460),
//! on top of #955/#1454's `crate::core::review` and #954's
//! `Engine::acp_write_text_file`: recording every file a turn writes,
//! opening one combined turn-review surface once the turn ends (or on
//! `:AiReview`), and restoring a past turn's checkpoint (`:AiRestore`).
//! `crate::core::acp_turn` itself stays free of any `Engine`/buffer/
//! filesystem knowledge — this module is the only place that bridges the
//! two, mirroring the `crate::core::review` / `review_ops.rs` split.
//!
//! ## Turn review vs proposal review: same surface, opposite `a`/`r`
//!
//! [`crate::core::review::ChangeReviewState`] already renders "pre vs
//! post" for a list of files — #1460 reuses it rather than building a
//! second `DiffView`-based surface. But a *turn* review's files are
//! already written to disk (they went through `fs/write_text_file`
//! already), unlike a *proposal* review's `diff` blocks (#955), which are
//! not yet applied — so the two need opposite `a`/`r` semantics:
//!
//! | Key | Proposal review (#955)              | Turn review (#1460)               |
//! |-----|--------------------------------------|-------------------------------------|
//! | `a` | writes `new_text` to disk            | no-op decision — already correct    |
//! | `r` | no buffer effect                     | reverts to the pre-turn content      |
//!
//! [`Engine::handle_change_review_key`] (`review_ops.rs`) branches on
//! [`Engine::turn_review_checkpoint_id`] to pick which of the two a given
//! keypress means — `Some(_)` while a turn review is open, `None` for a
//! proposal review, never both at once (opening either kind resets the
//! other's marker).

use super::*;
use crate::core::acp_turn::{AcpTurnCheckpoint, RestorePlanItem};
use crate::core::review::ProposedChange;

impl Engine {
    /// Record one `fs/write_text_file` write into the in-flight turn's
    /// tracking (#1460) — called from [`Self::acp_write_text_file`] just
    /// before it overwrites the buffer, `pre_content` being what the file
    /// held immediately before this write, or `None` if the path did not
    /// exist on disk yet (the agent is creating it) — see
    /// [`crate::core::acp_turn::AcpTurnFileEntry`]'s doc for why that
    /// distinction matters on revert. A no-op on the *checkpoint* itself
    /// for a path already touched this turn (only the "last written"
    /// value moves forward) — see
    /// [`crate::core::acp_turn::AcpTurnCheckpoint::record_write`].
    pub(crate) fn acp_record_turn_write(
        &mut self,
        path: &str,
        pre_content: Option<String>,
        written_content: String,
    ) {
        if let Some(existing) = self
            .acp_current_turn_entries
            .iter_mut()
            .find(|e| e.path == path)
        {
            existing.agent_written_content = written_content;
        } else {
            self.acp_current_turn_entries
                .push(crate::core::acp_turn::AcpTurnFileEntry {
                    path: path.to_string(),
                    pre_turn_content: pre_content,
                    agent_written_content: written_content,
                });
        }
    }

    /// End the in-flight turn (`AcpEvent::PromptStopped`): roll whatever it
    /// wrote into a fresh [`AcpTurnCheckpoint`] and open the combined turn-
    /// review surface for it. A no-op if the turn wrote nothing — nothing
    /// to review, nothing to checkpoint.
    pub(crate) fn acp_end_turn(&mut self) {
        if self.acp_current_turn_entries.is_empty() {
            return;
        }
        self.acp_turn_checkpoint_counter += 1;
        let id = self.acp_turn_checkpoint_counter;
        let entries = std::mem::take(&mut self.acp_current_turn_entries);
        self.acp_turn_checkpoints
            .push(AcpTurnCheckpoint { id, entries });
        self.acp_open_turn_review(id);
    }

    /// Open the turn-review surface for `checkpoint_id` — one
    /// [`crate::core::review::ChangeReviewState`] entry per file the turn touched, pre-turn
    /// content vs whatever the file holds *now* (buffer-first, same read
    /// [`Self::acp_current_file_content`] uses for a proposal review). A
    /// no-op if `checkpoint_id` doesn't name a real checkpoint (already
    /// restored away, say).
    pub(crate) fn acp_open_turn_review(&mut self, checkpoint_id: usize) {
        let Some(checkpoint) = self
            .acp_turn_checkpoints
            .iter()
            .find(|c| c.id == checkpoint_id)
        else {
            return;
        };
        let changes: Vec<ProposedChange> = checkpoint
            .entries
            .iter()
            .map(|e| {
                let now = self
                    .acp_current_file_content(std::path::Path::new(&e.path))
                    .unwrap_or_else(|_| e.agent_written_content.clone());
                ProposedChange {
                    path: e.path.clone(),
                    // `None` here (a path the agent created, no prior
                    // content) makes the review render it as a pure
                    // addition, same as a `diff` block on a brand-new
                    // path (#955) — see `AcpTurnFileEntry`'s doc.
                    old_text: e.pre_turn_content.clone(),
                    new_text: now,
                }
            })
            .collect();
        self.open_change_review_with_turn_marker(changes, Some(checkpoint_id));
    }

    /// Forget `paths`' bookkeeping in every checkpoint at or after
    /// `target_id` once those paths' agent writes have actually been
    /// reverted back to disk — shared by [`Self::acp_restore_checkpoint`]
    /// (a whole-checkpoint restore) and
    /// [`crate::core::engine::review_ops::Engine::change_review_reject_current`]'s
    /// turn-review branch (a single-file revert via the `r` key on an open
    /// turn review).
    ///
    /// Two things this buys, both from the review's findings on #1460:
    /// - **A checkpoint survives a refusal.** Only the paths that were
    ///   actually reverted are dropped, so a checkpoint with any refused
    ///   (human-edited-since) file keeps that file's entry and remains in
    ///   [`Self::acp_turn_checkpoints`] for a later retry — the whole point
    ///   of a checkpoint as a rollback point. A checkpoint disappears only
    ///   once *every* entry it holds has been reverted.
    /// - **No stale `agent_written_content`.** A single-file revert through
    ///   the turn-review `r` key writes the pre-turn content back via
    ///   [`Self::acp_write_file_untracked`], which — by design — does not
    ///   touch this bookkeeping. Without this call, a later `:AiRestore`
    ///   would compare the now-reverted disk content against the stale
    ///   `agent_written_content` still on file and wrongly report the
    ///   already-reverted path as "refused (edited since)".
    pub(crate) fn acp_forget_reverted_checkpoint_paths(
        &mut self,
        target_id: usize,
        paths: &[String],
    ) {
        if paths.is_empty() {
            return;
        }
        for cp in self.acp_turn_checkpoints.iter_mut() {
            if cp.id >= target_id {
                cp.entries.retain(|e| !paths.contains(&e.path));
            }
        }
        self.acp_turn_checkpoints
            .retain(|c| c.id < target_id || !c.entries.is_empty());
    }

    /// `:AiReview` — open the turn-review surface for the most recently
    /// completed turn (#1460's "or on `:AiReview`" alternative to the
    /// automatic `PromptStopped` open). A status message, not an error,
    /// when there is nothing to review yet — this is a look-around
    /// command, not a state-changing one.
    pub(crate) fn cmd_ai_review(&mut self) {
        let Some(id) = self.acp_turn_checkpoints.last().map(|c| c.id) else {
            self.message = "No ACP turn changes to review yet".to_string();
            return;
        };
        self.acp_open_turn_review(id);
    }

    /// `:AiRestore [id]` — revert every file touched by checkpoint `id` (or
    /// the most recent checkpoint, with no argument) and every later
    /// checkpoint's writes to it (#1460's "reverts every later agent
    /// write"), each through the single-undo-group buffer path
    /// ([`Self::acp_write_file_untracked`]).
    ///
    /// A file whose current content no longer matches what the agent last
    /// wrote it (a human edit landed on it since — see
    /// [`crate::core::acp_turn::plan_restore`]'s doc) is refused rather
    /// than clobbered; the summary names every reverted and every refused
    /// path so the human immediately sees which files need a manual look.
    ///
    /// Only the paths that were *actually* reverted are forgotten from
    /// [`Self::acp_turn_checkpoints`] (via
    /// [`Self::acp_forget_reverted_checkpoint_paths`]) — a refused path
    /// keeps its checkpoint entry intact so a later retry (once the human
    /// edit is dealt with) still has something to restore *to*. A
    /// checkpoint only disappears once every one of its entries has been
    /// reverted; a checkpoint with any refused file remains in history,
    /// exactly the "checkpoint as a rollback point" contract a fully
    /// destroyed-on-refusal checkpoint would otherwise break.
    pub fn acp_restore_checkpoint(&mut self, id: Option<usize>) -> Result<String, String> {
        let target_id = match id {
            Some(id) => id,
            None => self
                .acp_turn_checkpoints
                .last()
                .map(|c| c.id)
                .ok_or_else(|| "no ACP turn checkpoints to restore".to_string())?,
        };
        if !self.acp_turn_checkpoints.iter().any(|c| c.id == target_id) {
            return Err(format!("no such ACP turn checkpoint: {target_id}"));
        }
        let checkpoints = self.acp_turn_checkpoints.clone();
        let plan = crate::core::acp_turn::plan_restore(&checkpoints, target_id, |path| {
            self.acp_current_file_content(std::path::Path::new(path))
                .ok()
        });

        let mut reverted = Vec::new();
        let mut refused = Vec::new();
        for item in &plan {
            match item {
                RestorePlanItem::Revert {
                    pre_turn_content, ..
                } => {
                    let path = item.path().to_string();
                    // `None` means the agent created this path — "revert"
                    // means delete it, not write an empty file (#1460
                    // review non-blocking finding; see
                    // `AcpTurnFileEntry`'s doc).
                    let result = match pre_turn_content {
                        Some(content) => {
                            self.acp_write_file_untracked(std::path::Path::new(&path), content)
                        }
                        None => self.acp_delete_file_untracked(std::path::Path::new(&path)),
                    };
                    if let Err(msg) = result {
                        return Err(format!("failed to restore {path}: {msg}"));
                    }
                    reverted.push(path);
                }
                RestorePlanItem::Refused { reason, .. } => {
                    refused.push(format!("{} ({reason})", item.path()));
                }
            }
        }
        self.acp_forget_reverted_checkpoint_paths(target_id, &reverted);
        if self
            .turn_review_checkpoint_id
            .is_some_and(|open_id| open_id >= target_id)
        {
            self.change_review = None;
            self.turn_review_checkpoint_id = None;
        }

        let mut summary = if reverted.is_empty() {
            format!("Restored checkpoint {target_id}: nothing to revert")
        } else {
            format!(
                "Restored checkpoint {target_id}: reverted {}",
                reverted.join(", ")
            )
        };
        if !refused.is_empty() {
            summary.push_str(&format!(
                " \u{2014} refused (edited since): {}",
                refused.join(", ")
            ));
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unique scratch dir per test — same manual pattern `review_ops.rs`'s
    /// own `unique_temp_dir` uses.
    fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "acp-turn-ops-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// #1460's own words: "the turn review lists all 3" — a fake agent
    /// (here: three direct `acp_write_text_file` calls, the exact call
    /// `fs/write_text_file` handling makes — see `acp_ops.rs`'s own
    /// `acp_write_text_file_*` tests for the same style) writing three
    /// files in one turn must open a review whose entries name all three,
    /// each with its pre-turn content as `old_text`.
    #[test]
    fn ending_a_turn_that_wrote_three_files_lists_all_three_in_the_review() {
        let dir = unique_temp_dir("three-files");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        let c = dir.join("c.txt");
        std::fs::write(&a, "orig a\n").unwrap();
        std::fs::write(&b, "orig b\n").unwrap();
        std::fs::write(&c, "orig c\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_write_text_file(&a, "new a\n").unwrap();
        engine.acp_write_text_file(&b, "new b\n").unwrap();
        engine.acp_write_text_file(&c, "new c\n").unwrap();
        assert_eq!(
            engine.acp_current_turn_entries.len(),
            3,
            "all three writes must be tracked before the turn ends"
        );

        engine.acp_end_turn();

        assert_eq!(
            engine.acp_turn_checkpoints.len(),
            1,
            "ending the turn must produce exactly one checkpoint"
        );
        let review = engine
            .change_review
            .as_ref()
            .expect("ending a turn that wrote files must open the review surface");
        assert_eq!(review.entries.len(), 3, "the review must list all 3 files");
        let mut paths: Vec<_> = review
            .entries
            .iter()
            .map(|e| e.change.path.clone())
            .collect();
        paths.sort();
        let mut expected: Vec<_> = [&a, &b, &c]
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        expected.sort();
        assert_eq!(paths, expected);

        let a_entry = review
            .entries
            .iter()
            .find(|e| e.change.path == a.to_string_lossy())
            .unwrap();
        assert_eq!(a_entry.change.old_text.as_deref(), Some("orig a\n"));
        assert_eq!(a_entry.change.new_text, "new a\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ending a turn that wrote nothing must not open an empty review or
    /// fabricate a checkpoint.
    #[test]
    fn ending_a_turn_with_no_writes_is_a_noop() {
        let mut engine = Engine::new_for_test();
        engine.acp_end_turn();
        assert!(engine.acp_turn_checkpoints.is_empty());
        assert!(engine.change_review.is_none());
    }

    /// #1460: "reverting one restores exactly its pre-turn contents" —
    /// pressing `r` (reject, repurposed as revert in turn-review mode) on
    /// one file writes its `old_text` back to disk, leaving the other
    /// file(s) untouched.
    #[test]
    fn revert_one_file_in_a_turn_review_restores_exactly_its_pre_turn_contents() {
        let dir = unique_temp_dir("revert-one");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        std::fs::write(&a, "orig a\n").unwrap();
        std::fs::write(&b, "orig b\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_write_text_file(&a, "new a\n").unwrap();
        engine.acp_write_text_file(&b, "new b\n").unwrap();
        engine.acp_end_turn();
        assert!(engine.turn_review_checkpoint_id.is_some());

        // The review starts on entry 0 (`a.txt`, insertion order).
        engine.handle_change_review_key("", Some('r'));

        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            "orig a\n",
            "reverting a.txt must restore exactly its pre-turn content"
        );
        assert_eq!(
            std::fs::read_to_string(&b).unwrap(),
            "new b\n",
            "the other file must be untouched by reverting a.txt alone"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1460 review (non-blocking finding): reverting a file the agent
    /// *created* this turn (didn't exist beforehand) must delete it, not
    /// leave an empty file behind — RED against the unfixed
    /// `unwrap_or_default()` path (which wrote back `""`): this assertion
    /// would see the file still present with empty content instead of
    /// `!exists()`.
    #[test]
    fn revert_a_file_the_agent_created_this_turn_deletes_it_rather_than_emptying_it() {
        let dir = unique_temp_dir("revert-created-file");
        let new_file = dir.join("brand-new.txt");
        assert!(!new_file.exists(), "sanity: must not exist yet");

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine
            .acp_write_text_file(&new_file, "created by agent\n")
            .unwrap();
        engine.acp_end_turn();
        assert!(engine.turn_review_checkpoint_id.is_some());

        engine.handle_change_review_key("", Some('r'));

        assert!(
            !new_file.exists(),
            "reverting an agent-created file must delete it, not leave it emptied"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Same as above, but through `:AiRestore`'s whole-checkpoint path
    /// rather than a single-file `r` in the review UI.
    #[test]
    fn restore_checkpoint_deletes_a_file_the_agent_created_this_turn() {
        let dir = unique_temp_dir("restore-created-file");
        let new_file = dir.join("brand-new.txt");

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine
            .acp_write_text_file(&new_file, "created by agent\n")
            .unwrap();
        engine.acp_end_turn();
        let checkpoint_id = engine.turn_review_checkpoint_id.unwrap();

        let summary = engine.acp_restore_checkpoint(Some(checkpoint_id)).unwrap();

        assert!(
            !new_file.exists(),
            ":AiRestore must delete an agent-created file, not empty it: {summary}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The `a`/keep key in turn-review mode must be a pure decision — no
    /// buffer write, since the file is already correct.
    #[test]
    fn keep_one_file_in_a_turn_review_does_not_touch_disk() {
        let dir = unique_temp_dir("keep-one");
        let a = dir.join("a.txt");
        std::fs::write(&a, "orig a\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_write_text_file(&a, "new a\n").unwrap();
        engine.acp_end_turn();

        engine.handle_change_review_key("", Some('a'));

        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            "new a\n",
            "keeping a turn-review entry must leave the agent's write in place"
        );
        assert!(
            engine.change_review.is_none(),
            "keeping the only entry auto-closes the surface"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1460: "restoring a checkpoint undoes the agent's later writes but
    /// keeps a user edit made in between." Turn 1 writes `a.txt`; the user
    /// then hand-edits `a.txt` directly (not through the agent); restoring
    /// checkpoint 1 must refuse to touch `a.txt` (leaving the user's edit
    /// intact) while still reverting `b.txt`, which nothing touched after
    /// the agent wrote it.
    #[test]
    fn restore_checkpoint_keeps_a_user_edit_made_in_between() {
        let dir = unique_temp_dir("restore-keeps-user-edit");
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        std::fs::write(&a, "orig a\n").unwrap();
        std::fs::write(&b, "orig b\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_write_text_file(&a, "agent a\n").unwrap();
        engine.acp_write_text_file(&b, "agent b\n").unwrap();
        engine.acp_end_turn();
        let checkpoint_id = engine.turn_review_checkpoint_id.unwrap();
        // Close the review without deciding anything — restoring should
        // work against a closed-but-checkpointed turn too.
        engine.close_change_review();

        // The user hand-edits a.txt directly on disk (simulating a manual
        // `:w` after editing in the buffer) — NOT through the agent.
        std::fs::write(&a, "human edit of a\n").unwrap();
        // Reflect it in the open buffer too, matching how a real edit
        // would leave both buffer and disk in sync.
        let buf_id = engine.buffer_manager.open_file(&a).unwrap();
        {
            let state = engine.buffer_manager.get_mut(buf_id).unwrap();
            let len = state.buffer.content.len_chars();
            state.buffer.content.remove(0..len);
            state.buffer.content.insert(0, "human edit of a\n");
        }

        let summary = engine.acp_restore_checkpoint(Some(checkpoint_id)).unwrap();

        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            "human edit of a\n",
            "the user's edit to a.txt must survive the restore untouched"
        );
        assert_eq!(
            std::fs::read_to_string(&b).unwrap(),
            "orig b\n",
            "b.txt, which nothing touched after the agent wrote it, must revert"
        );
        assert!(
            summary.contains("b.txt") || summary.contains(&b.to_string_lossy().into_owned()),
            "summary should name the reverted file: {summary}"
        );
        assert!(
            summary.to_lowercase().contains("refused") || summary.to_lowercase().contains("edited"),
            "summary should call out the refused file: {summary}"
        );

        // #1460 review (blocking finding): a refused restore must not
        // destroy the checkpoint — only the *reverted* path (b.txt) is
        // forgotten; a.txt's refused entry must remain so a later retry
        // (once the human edit is dealt with) still has something to
        // restore. RED against the unfixed
        // `self.acp_turn_checkpoints.retain(|c| c.id < target_id)` (which
        // dropped checkpoint 1 wholesale here, regardless of a.txt's
        // refusal): this assertion failed with an empty checkpoint list.
        assert_eq!(
            engine.acp_turn_checkpoints.len(),
            1,
            "checkpoint {checkpoint_id} must survive a partially-refused restore, not be destroyed"
        );
        let surviving = &engine.acp_turn_checkpoints[0];
        assert_eq!(surviving.id, checkpoint_id);
        assert_eq!(
            surviving.entries.len(),
            1,
            "only a.txt's refused entry should remain — b.txt's was actually reverted"
        );
        assert_eq!(surviving.entries[0].path, a.to_string_lossy());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1460 review (blocking finding): once the human edit that caused a
    /// refusal is discarded, a *later* `:AiRestore` retry against the same
    /// checkpoint must actually work — proving the surviving checkpoint
    /// from the test above isn't just present but inert. RED against the
    /// unfixed blanket `retain`: the first restore already destroyed the
    /// checkpoint, so this second call fails with "no such ACP turn
    /// checkpoint" instead of reverting.
    #[test]
    fn restore_checkpoint_can_be_retried_after_the_human_edit_is_discarded() {
        let dir = unique_temp_dir("restore-retry-after-refusal");
        let a = dir.join("a.txt");
        std::fs::write(&a, "orig a\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_write_text_file(&a, "agent a\n").unwrap();
        engine.acp_end_turn();
        let checkpoint_id = engine.turn_review_checkpoint_id.unwrap();
        engine.close_change_review();

        // A human edit lands on top of the agent's write — first restore
        // must refuse it. `acp_current_file_content` reads buffer-first
        // (same as production `fs/read_text_file`), so the edit has to
        // land in the buffer too, not just on disk, to be seen as a human
        // edit rather than the agent's own still-buffered write.
        std::fs::write(&a, "human edit\n").unwrap();
        let buf_id = engine.buffer_manager.open_file(&a).unwrap();
        {
            let state = engine.buffer_manager.get_mut(buf_id).unwrap();
            let len = state.buffer.content.len_chars();
            state.buffer.content.remove(0..len);
            state.buffer.content.insert(0, "human edit\n");
        }
        let first = engine.acp_restore_checkpoint(Some(checkpoint_id)).unwrap();
        assert!(
            first.to_lowercase().contains("refused"),
            "first restore must refuse: {first}"
        );
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "human edit\n");
        assert!(
            engine
                .acp_turn_checkpoints
                .iter()
                .any(|c| c.id == checkpoint_id),
            "checkpoint must still exist after the refusal for a retry"
        );

        // The human discards their edit, putting the file back to exactly
        // what the agent last wrote — matching `agent_written_content`
        // again, so a retry should now succeed.
        std::fs::write(&a, "agent a\n").unwrap();
        let buf_id = engine.buffer_manager.open_file(&a).unwrap();
        {
            let state = engine.buffer_manager.get_mut(buf_id).unwrap();
            let len = state.buffer.content.len_chars();
            state.buffer.content.remove(0..len);
            state.buffer.content.insert(0, "agent a\n");
        }

        let second = engine.acp_restore_checkpoint(Some(checkpoint_id)).unwrap();
        assert!(
            !second.to_lowercase().contains("refused"),
            "retry after discarding the human edit must succeed: {second}"
        );
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            "orig a\n",
            "the retried restore must actually revert to the pre-turn content"
        );
        assert!(
            engine.acp_turn_checkpoints.is_empty(),
            "a fully-reverted checkpoint is forgotten once nothing is left refused"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A restore with no argument targets the most recent checkpoint.
    #[test]
    fn restore_checkpoint_defaults_to_the_most_recent() {
        let dir = unique_temp_dir("restore-default");
        let a = dir.join("a.txt");
        std::fs::write(&a, "orig a\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_write_text_file(&a, "turn1 a\n").unwrap();
        engine.acp_end_turn();

        let summary = engine.acp_restore_checkpoint(None).unwrap();
        assert!(summary.contains('1'), "unexpected summary: {summary}");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "orig a\n");
        assert!(engine.acp_turn_checkpoints.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_checkpoint_errors_with_no_checkpoints() {
        let mut engine = Engine::new_for_test();
        let err = engine.acp_restore_checkpoint(None).unwrap_err();
        assert!(err.contains("no ACP turn checkpoints"));
    }

    #[test]
    fn cmd_ai_review_reopens_the_most_recent_checkpoint() {
        let dir = unique_temp_dir("ai-review-cmd");
        let a = dir.join("a.txt");
        std::fs::write(&a, "orig a\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_write_text_file(&a, "new a\n").unwrap();
        engine.acp_end_turn();
        engine.close_change_review();
        assert!(engine.change_review.is_none());

        engine.cmd_ai_review();
        assert!(engine.change_review.is_some());
        assert_eq!(engine.change_review.as_ref().unwrap().entries.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cmd_ai_review_with_nothing_to_review_leaves_a_message() {
        let mut engine = Engine::new_for_test();
        engine.cmd_ai_review();
        assert!(engine.change_review.is_none());
        assert!(engine.message.contains("No ACP turn changes"));
    }

    // ── :AiReview / :AiRestore ex commands (#1460) ──────────────────────────

    #[test]
    fn ai_review_ex_command_reopens_the_most_recent_checkpoint() {
        let dir = unique_temp_dir("ai-review-ex");
        let a = dir.join("a.txt");
        std::fs::write(&a, "orig a\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_write_text_file(&a, "new a\n").unwrap();
        engine.acp_end_turn();
        engine.close_change_review();
        assert!(engine.change_review.is_none());

        let action = engine.execute_command("AiReview");
        assert_ne!(action, EngineAction::Error);
        assert!(engine.change_review.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ai_restore_ex_command_with_no_argument_restores_the_most_recent_checkpoint() {
        let dir = unique_temp_dir("ai-restore-ex-default");
        let a = dir.join("a.txt");
        std::fs::write(&a, "orig a\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_write_text_file(&a, "turn1 a\n").unwrap();
        engine.acp_end_turn();

        let action = engine.execute_command("AiRestore");
        assert_ne!(
            action,
            EngineAction::Error,
            "AiRestore must be a recognised command: {}",
            engine.message
        );
        assert!(engine.message.contains("Restored checkpoint"));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "orig a\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ai_restore_ex_command_with_an_explicit_id_restores_that_checkpoint() {
        let dir = unique_temp_dir("ai-restore-ex-id");
        let a = dir.join("a.txt");
        std::fs::write(&a, "orig a\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_write_text_file(&a, "turn1 a\n").unwrap();
        engine.acp_end_turn();
        let checkpoint_id = engine.turn_review_checkpoint_id.unwrap();

        let action = engine.execute_command(&format!("AiRestore {checkpoint_id}"));
        assert_ne!(
            action,
            EngineAction::Error,
            "unexpected: {}",
            engine.message
        );
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "orig a\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ai_restore_ex_command_with_no_checkpoints_errors() {
        let mut engine = Engine::new_for_test();
        let action = engine.execute_command("AiRestore");
        assert_eq!(action, EngineAction::Error);
        assert!(engine.message.contains("no ACP turn checkpoints"));
    }

    #[test]
    fn ai_restore_ex_command_rejects_a_non_numeric_argument() {
        let mut engine = Engine::new_for_test();
        let action = engine.execute_command("AiRestore notanumber");
        assert_eq!(action, EngineAction::Error);
        assert!(engine.message.contains("Usage: AiRestore"));
    }
}
