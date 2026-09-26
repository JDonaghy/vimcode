//! `Engine` wiring for the change-review surface (#955, shared with #525):
//! opening/closing, keyboard navigation, accept/reject application, and
//! location resolution. `crate::core::review` itself stays free of any
//! `Engine`/buffer/backend knowledge — this module is the only place that
//! bridges the two, mirroring the `crate::core::acp` / `acp_ops.rs` split.
//!
//! ## Mouse click-to-jump: both the logic and the wiring ship here
//!
//! [`Engine::change_review_jump_to_hit`] resolves a real
//! `quadraui::DiffViewHit` (from `DiffView::layout(..).hit_test(x, y)`)
//! to a `(path, line)` and jumps there — the piece "clicking a location
//! jumps to that file and line" needs, closing the surface on success so
//! the jumped-to buffer is what paints next rather than the (full-viewport)
//! diff. Both backends' mouse handling routes a real click into this
//! overlay *before* the modal-overlay ladder, the same "swallows every
//! click while open" precedent the folder picker set: GTK's
//! `App::route_and_apply_change_review_click` (`src/app.rs`) and TUI's
//! equivalent branch in `mouse::handle_mouse` (`src/tui_main/mouse.rs`)
//! both call the shared `render::route_change_review_click` to resolve the
//! click against the exact geometry `render::paint_change_review_rung`
//! last painted, then this function to act on it. The keyboard path
//! (`Return` in [`Engine::handle_change_review_key`]) exercises the same
//! resolution function.

use super::*;
use crate::core::review::{row_to_location, ProposedChange};
use crate::core::tool_client::BranchReviewTarget;

impl Engine {
    /// Open the change-review surface for `changes`, replacing whatever
    /// was open before. A no-op on an empty list — nothing to review.
    /// Public so a future source-agnostic feeder (a #525 git-branch diff
    /// list, say) can call it directly without going through ACP at all.
    pub fn open_change_review(&mut self, changes: Vec<ProposedChange>) {
        if changes.is_empty() {
            return;
        }
        self.change_review = Some(crate::core::review::ChangeReviewState::new(changes));
        // Always reset first — only `Engine::open_review_card` (board_ops.rs,
        // #526) knows the reviewed card's id and sets it back afterward. An
        // ACP tool-call diff (or a direct `open_branch_review` call with no
        // board card behind it) must never inherit a stale id left over
        // from an earlier board review.
        self.review_card_id = None;
    }

    /// Build a source-agnostic change list from a local git branch diff
    /// (#525) and open it via [`Self::open_change_review`] — the git
    /// feeder for the surface #955 built, proving it really is source-
    /// agnostic (see `crate::core::review`'s own non-ACP unit test for the
    /// same point at the data-model level). `target` is typically resolved
    /// from a board card through a provider's `"OpenReview"` action
    /// (`Engine::open_review_card` in `board_ops.rs`) — this method itself
    /// has no idea where `target` came from, it just turns two git
    /// revisions into a diff.
    ///
    /// Worktree-local (#525's stated scope): both `target.branch` and
    /// `target.base` must already be resolvable in the local repository —
    /// still no fetch, no `ssh` handling, even after #530 (Track A Phase
    /// 5). That issue's "review-where-the-code-is" mode is *vimcode itself
    /// running on the worker box*, at which point `target.branch`/`.base`
    /// are trivially local again — the only new data this method threads
    /// through unexamined is `target.host`, purely for the provenance
    /// footer (`render::paint_change_review_rung`) to show which machine
    /// that local checkout happens to be on.
    pub fn open_branch_review(&mut self, target: BranchReviewTarget) -> Result<(), String> {
        let root = self
            .workspace_root
            .clone()
            .ok_or_else(|| "no workspace open".to_string())?;
        // Recorded *before* the diff build can fail below, and left set
        // even on that failure: a provider that resolved a real
        // branch/base the local repo can't currently diff (wrong worktree,
        // stale fetch) is exactly the provenance a human is about to need
        // while they go figure out why, not something to silently drop.
        self.review_target = Some(target.clone());
        // `changed_files_between` now distinguishes "git failure" (`None`
        // — unknown ref, bad revision, no repo) from "a real, empty diff"
        // (`Some(vec![])`), so a misconfigured provider's bogus branch/base
        // name surfaces as its own message rather than silently reading
        // the same as "nothing changed" (review non-blocking finding,
        // #525).
        let paths =
            match crate::core::git::changed_files_between(&root, &target.base, &target.branch) {
                Some(paths) if !paths.is_empty() => paths,
                Some(_) => {
                    return Err(format!(
                        "no changes between '{}' and '{}'",
                        target.base, target.branch
                    ))
                }
                None => {
                    return Err(format!(
                        "could not diff '{}'...'{}' — check the branch/base names \
                         the provider returned",
                        target.base, target.branch
                    ))
                }
            };
        let changes = paths
            .into_iter()
            .map(|path| {
                let old_text = crate::core::git::show_file_at_ref(&root, &target.base, &path);
                let new_text = crate::core::git::show_file_at_ref(&root, &target.branch, &path)
                    .unwrap_or_default();
                ProposedChange {
                    path,
                    old_text,
                    new_text,
                }
            })
            .collect();
        self.open_change_review(changes);
        Ok(())
    }

    /// Commit any working-tree edits made while reviewing a branch, and
    /// push them — the "human edits, pushed back" leg of #528 (Track A
    /// Phase 3), modelled on coordinator's remote-fix `finalize`: a
    /// commit, then a plain (non-force) [`crate::core::git::push`], so a
    /// rejected push never loses anything — the commit, and the worktree
    /// it lives in, are left exactly where they are for a human to
    /// resolve and retry. Reachable as `:GFinalize [message]` (`execute.
    /// rs`); `message` defaults to `"Review edits"` when omitted.
    ///
    /// Safety check: if a branch review was opened this session
    /// (`self.review_target`, set by [`Self::open_branch_review`] and left
    /// in place after the diff surface itself is closed — see that
    /// field's own doc), refuses to push unless the worktree is *still*
    /// on that exact branch. Nothing stops a human from `git checkout`ing
    /// elsewhere in the same worktree mid-review and then hitting
    /// finalize, which would otherwise push edits meant for the reviewed
    /// branch onto whatever branch happens to be checked out — precisely
    /// the "must know which branch they're editing" footgun the issue
    /// calls out. No review opened this session (`review_target: None`)
    /// — finalize is then a generic "commit and push whatever's here",
    /// same as `:Gcommit` + `:Gpush` already are, so there is nothing to
    /// check against.
    pub fn finalize_review_edits(&mut self, message: &str) -> Result<String, String> {
        let dir = self
            .workspace_root
            .clone()
            .unwrap_or_else(|| self.git_dir());
        if let Some(target) = &self.review_target {
            let current = crate::core::git::current_branch(&dir);
            if current.as_deref() != Some(target.branch.as_str()) {
                return Err(format!(
                    "refusing to finalize: worktree is on '{}', not the reviewed branch '{}' \
                     — checkout '{}' first",
                    current.as_deref().unwrap_or("(detached/unknown)"),
                    target.branch,
                    target.branch,
                ));
            }
        }
        let has_changes = !crate::core::git::status_detailed(&dir).is_empty();
        if has_changes {
            crate::core::git::stage_all(&dir)?;
            crate::core::git::commit(&dir, message)?;
            let ids: Vec<_> = self.buffer_manager.list();
            for id in ids {
                self.refresh_git_diff(id);
            }
            self.git_branch = crate::core::git::current_branch(&dir);
        }
        match crate::core::git::push(&dir) {
            Ok(summary) => Ok(if has_changes {
                format!(
                    "Finalized review edits and pushed: {}",
                    if summary.is_empty() {
                        "ok".to_string()
                    } else {
                        summary
                    }
                )
            } else if summary.is_empty() {
                "Nothing to commit; branch already up to date with origin.".to_string()
            } else {
                summary
            }),
            Err(e) => Err(format!(
                "push failed — commit and worktree preserved, retry with :GFinalize once \
                 resolved: {e}"
            )),
        }
    }

    /// Close the change-review surface without deciding anything left
    /// pending (Esc). Entries already accepted/rejected keep their effect
    /// — closing only discards the *surface*, not decisions already made.
    pub fn close_change_review(&mut self) {
        self.change_review = None;
    }

    /// Handle a keypress while the change-review surface is open. Returns
    /// `true` if the key was consumed — every key while the surface is
    /// open is consumed (mirroring `handle_diff_peek_key`'s "any other
    /// key" fallback, just without the fall-through-and-close case, since
    /// there is no single "primary" action here to default to).
    ///
    /// | Key(s)      | Action                                   |
    /// |-------------|-------------------------------------------|
    /// | Esc / `q`   | Close the surface                          |
    /// | `j` / Down  | Scroll down one row                        |
    /// | `k` / Up    | Scroll up one row                          |
    /// | `]`         | Jump to the next hunk                      |
    /// | `[`         | Jump to the previous hunk                  |
    /// | `n` / Tab   | Next file                                  |
    /// | `p`         | Previous file                              |
    /// | `a`         | Accept the current file's change           |
    /// | `r`         | Reject the current file's change           |
    /// | `A`         | Approve — report a verdict (#526)          |
    /// | `C`         | Request changes — report a verdict (#526)  |
    /// | `M`         | Comment-only — report a verdict (#526)     |
    /// | `c`         | Add/edit a comment on the current line (#527) |
    /// | `d`         | Delete the comment on the current line (#527) |
    ///
    /// The three verdict keys hand off to
    /// [`Self::start_review_verdict`], which closes this surface and opens
    /// a body-composer buffer — see `review_verdict_ops.rs`'s module doc.
    /// A key with no provider-configured command for that verdict (or no
    /// reviewed card behind this surface at all — an ACP tool-call diff,
    /// say) degrades to a status message rather than doing nothing
    /// silently.
    pub(crate) fn handle_change_review_key(
        &mut self,
        key_name: &str,
        unicode: Option<char>,
    ) -> bool {
        match key_name {
            "Escape" | "q" => {
                self.close_change_review();
                return true;
            }
            "Down" => {
                self.change_review_scroll(1);
                return true;
            }
            "Up" => {
                self.change_review_scroll(-1);
                return true;
            }
            "Tab" => {
                if let Some(review) = &mut self.change_review {
                    review.next_file();
                }
                return true;
            }
            // "Clicking a location jumps to that file and line" (#955's
            // acceptance bar): `Engine::change_review_jump_to_hit` already
            // resolves a real mouse `DiffViewHit` against the surface's
            // geometry, but neither backend's mouse-arbitration ladder
            // routes clicks into this overlay yet (a follow-up — see this
            // module's top doc). Enter is the keyboard-reachable
            // equivalent in the meantime: jump to the file/line at the
            // row the view is currently scrolled to, exactly the same
            // resolution a click on that row would produce.
            "Return" | "Enter" => {
                let row_idx = self
                    .change_review
                    .as_ref()
                    .and_then(|r| r.current_entry())
                    .map(|e| e.view.scroll_offset)
                    .unwrap_or(0);
                self.change_review_jump_to_hit(quadraui::DiffViewHit::Row {
                    row_idx,
                    pane: None,
                });
                self.close_change_review();
                return true;
            }
            _ => {}
        }
        match unicode {
            Some('j') => self.change_review_scroll(1),
            Some('k') => self.change_review_scroll(-1),
            Some(']') => {
                if let Some(review) = &mut self.change_review {
                    review.next_hunk();
                }
            }
            Some('[') => {
                if let Some(review) = &mut self.change_review {
                    review.prev_hunk();
                }
            }
            Some('n') => {
                if let Some(review) = &mut self.change_review {
                    review.next_file();
                }
            }
            Some('p') => {
                if let Some(review) = &mut self.change_review {
                    review.prev_file();
                }
            }
            Some('a') => self.change_review_accept_current(),
            Some('r') => self.change_review_reject_current(),
            Some('A') => self.start_review_verdict(crate::core::review::ReviewVerdict::Approve),
            Some('C') => {
                self.start_review_verdict(crate::core::review::ReviewVerdict::RequestChanges)
            }
            Some('M') => self.start_review_verdict(crate::core::review::ReviewVerdict::Comment),
            Some('c') => self.change_review_start_comment(),
            Some('d') => self.change_review_delete_comment_at_current_line(),
            _ => {}
        }
        true
    }

    fn change_review_scroll(&mut self, delta: i64) {
        let Some(review) = &mut self.change_review else {
            return;
        };
        let Some(entry) = review.current_entry_mut() else {
            return;
        };
        entry.view.scroll_offset = if delta.is_negative() {
            entry
                .view
                .scroll_offset
                .saturating_sub(delta.unsigned_abs() as usize)
        } else {
            entry.view.scroll_offset.saturating_add(delta as usize)
        };
    }

    /// Accept the currently-shown entry: write `new_text` into a buffer
    /// for its `path` (opening or reusing one, undo-grouped, saved to
    /// disk) via the same buffer-write path `Engine::acp_write_text_file`
    /// uses for `fs/write_text_file` (#954), reused here rather than
    /// duplicated. Auto-closes the surface once every entry has a
    /// decision.
    ///
    /// Safe to write `new_text` as the buffer's entire content precisely
    /// because it's guaranteed whole-file by construction (#1454): every
    /// entry in `self.change_review` was built by
    /// `Engine::acp_open_review_for_diffs` -> `Engine::acp_resolve_diff_block`
    /// (`acp_ops.rs`), which resolves a possibly-fragment ACP `diff` block
    /// against the file's actual current content *before* a
    /// `ProposedChange` (and the `ChangeReviewEntry`/`DiffView` built from
    /// it) ever exists — an ambiguous or unlocatable fragment is refused
    /// there and never reaches an entry at all. This function itself does
    /// no fragment resolution; that would be the bug this issue reports
    /// (writing an edited-region fragment over the whole file).
    pub(crate) fn change_review_accept_current(&mut self) {
        let Some(review) = &mut self.change_review else {
            return;
        };
        let Some(change) = review.accept_current() else {
            return;
        };
        if let Err(msg) =
            self.acp_write_text_file(std::path::Path::new(&change.path), &change.new_text)
        {
            self.message = format!("Failed to apply change to {}: {msg}", change.path);
        } else {
            self.message = format!("Accepted change to {}", change.path);
        }
        if self.change_review.as_ref().is_some_and(|r| r.all_decided()) {
            self.change_review = None;
        }
    }

    /// Reject the currently-shown entry: no buffer effect. Auto-closes
    /// the surface once every entry has a decision.
    pub(crate) fn change_review_reject_current(&mut self) {
        let Some(review) = &mut self.change_review else {
            return;
        };
        review.reject_current();
        self.message = "Rejected change".to_string();
        if review.all_decided() {
            self.change_review = None;
        }
    }

    /// Resolve a click against the currently-shown entry's `DiffView` to a
    /// `(path, line)` and jump there — "clicking a location jumps to that
    /// file and line" (#955's acceptance bar), implemented against the
    /// diff surface's own row/hunk geometry (`DiffViewHit::Row`) rather
    /// than new transcript-click infrastructure quadraui doesn't ship yet.
    ///
    /// Closes the change-review surface on a successful jump (same as the
    /// keyboard `Return` path's explicit `close_change_review()`), since
    /// the surface is full-viewport on both backends — leaving it open
    /// would paint the diff right back over the buffer the jump just
    /// switched to, silently undoing the whole point of the click. A
    /// failed resolution (row not inside any hunk) leaves the surface
    /// open, mirroring `Return`'s own `row_to_location` failure case
    /// being a no-op rather than a forced close.
    pub(crate) fn change_review_jump_to_hit(&mut self, hit: quadraui::DiffViewHit) {
        let quadraui::DiffViewHit::Row { row_idx, .. } = hit else {
            return;
        };
        let Some((path, line)) = self
            .change_review
            .as_ref()
            .and_then(|r| r.current_entry())
            .and_then(|entry| row_to_location(entry, row_idx))
        else {
            return;
        };
        self.open_file_in_tab(std::path::Path::new(&path));
        let win_id = self.active_window_id();
        self.set_cursor_for_window(win_id, line.saturating_sub(1) as usize, 0);
        self.ensure_cursor_visible();
        self.close_change_review();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::review::ProposedChange;

    fn change(path: &str, old: Option<&str>, new: &str) -> ProposedChange {
        ProposedChange {
            path: path.to_string(),
            old_text: old.map(str::to_string),
            new_text: new.to_string(),
        }
    }

    #[test]
    fn open_change_review_is_a_noop_on_an_empty_list() {
        let mut engine = Engine::new_for_test();
        engine.open_change_review(vec![]);
        assert!(engine.change_review.is_none());
    }

    #[test]
    fn open_change_review_populates_the_surface() {
        let mut engine = Engine::new_for_test();
        engine.open_change_review(vec![change("f.rs", Some("a"), "b")]);
        assert!(engine.change_review.is_some());
        assert_eq!(
            engine.change_review.as_ref().unwrap().entries[0]
                .change
                .path,
            "f.rs"
        );
    }

    #[test]
    fn escape_closes_the_surface() {
        let mut engine = Engine::new_for_test();
        engine.open_change_review(vec![change("f.rs", Some("a"), "b")]);
        assert!(engine.handle_change_review_key("Escape", None));
        assert!(engine.change_review.is_none());
    }

    #[test]
    fn tab_and_n_advance_to_the_next_file() {
        let mut engine = Engine::new_for_test();
        engine.open_change_review(vec![
            change("a.rs", Some(""), "a"),
            change("b.rs", Some(""), "b"),
        ]);
        engine.handle_change_review_key("Tab", None);
        assert_eq!(engine.change_review.as_ref().unwrap().current, 1);
    }

    /// Unique scratch dir per test — same manual pattern
    /// `acp_ops.rs`'s own `unique_temp_dir` uses (no `tempfile` dependency
    /// in this crate).
    fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "review-ops-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ── open_branch_review (#525) ───────────────────────────────────────

    /// Set up a repo with a base commit, a `feature` branch carrying a
    /// modified file and a brand-new file, *and* a commit made on the base
    /// branch after `feature` diverged — proving `open_branch_review` uses
    /// three-dot semantics end to end (not just at the `git.rs` unit
    /// level): that base-only commit must never show up in the review.
    /// Returns `(repo dir, base branch name)`.
    fn init_branch_review_repo(tag: &str) -> (std::path::PathBuf, String) {
        let dir = unique_temp_dir(tag);
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init"]);
        git(&["config", "user.email", "t@t.com"]);
        git(&["config", "user.name", "T"]);
        std::fs::write(dir.join("modified.rs"), "before\n").unwrap();
        std::fs::write(dir.join("untouched.rs"), "same\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        git(&["branch", "feature"]);
        git(&["checkout", "feature"]);
        std::fs::write(dir.join("modified.rs"), "after\n").unwrap();
        std::fs::write(dir.join("added.rs"), "new file\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "feature work"]);
        let base = crate::core::git::current_branch(&dir).unwrap(); // still "feature" here
        let _ = base;
        let base_branch = if std::process::Command::new("git")
            .args(["rev-parse", "--verify", "main"])
            .current_dir(&dir)
            .output()
            .unwrap()
            .status
            .success()
        {
            "main"
        } else {
            "master"
        };
        git(&["checkout", base_branch]);
        std::fs::write(dir.join("untouched.rs"), "changed on base only\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base-only change"]);
        (dir, base_branch.to_string())
    }

    #[test]
    fn open_branch_review_builds_a_change_list_from_a_local_git_branch() {
        let (dir, base) = init_branch_review_repo("branch-review");
        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());

        engine
            .open_branch_review(BranchReviewTarget {
                branch: "feature".to_string(),
                base: base.clone(),
                host: None,
            })
            .expect("branch review should build a change list");

        // Provenance (#528): opening a branch review must record which
        // branch/base it resolved, so the UI (and `finalize_review_edits`'s
        // safety check) can tell the human which branch they're on.
        assert_eq!(
            engine.review_target,
            Some(BranchReviewTarget {
                branch: "feature".to_string(),
                base,
                host: None,
            }),
            "opening a branch review must record it as the review target"
        );

        let review = engine.change_review.as_ref().expect("surface should open");
        let mut paths: Vec<_> = review
            .entries
            .iter()
            .map(|e| e.change.path.clone())
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            vec!["added.rs".to_string(), "modified.rs".to_string()],
            "base-only 'untouched.rs' change must not appear (three-dot semantics)"
        );

        let added = review
            .entries
            .iter()
            .find(|e| e.change.path == "added.rs")
            .unwrap();
        assert!(
            added.change.old_text.is_none(),
            "a brand-new file has no left side"
        );
        assert_eq!(added.change.new_text, "new file\n");

        let modified = review
            .entries
            .iter()
            .find(|e| e.change.path == "modified.rs")
            .unwrap();
        assert_eq!(modified.change.old_text.as_deref(), Some("before\n"));
        assert_eq!(modified.change.new_text, "after\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_branch_review_errors_with_no_changes_between_revisions() {
        let (dir, base) = init_branch_review_repo("branch-review-noop");
        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());

        let err = engine
            .open_branch_review(BranchReviewTarget {
                branch: base.clone(),
                base,
                host: None,
            })
            .unwrap_err();
        assert!(err.contains("no changes"));
        assert!(engine.change_review.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_branch_review_errors_with_no_workspace_open() {
        let mut engine = Engine::new_for_test();
        engine.workspace_root = None;
        let err = engine
            .open_branch_review(BranchReviewTarget {
                branch: "feature".to_string(),
                base: "main".to_string(),
                host: None,
            })
            .unwrap_err();
        assert!(err.contains("no workspace"));
    }

    // ── finalize_review_edits / :GFinalize (#528, Track A Phase 3) ─────────

    /// Small `git -C dir <args>` runner shared by the finalize tests below
    /// — same shape as `init_branch_review_repo`'s local closure, just
    /// hoisted out since two tests need it against two different dirs.
    fn git_in(dir: &std::path::Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} (in {}) failed: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A working checkout with a real bare "origin" remote and an
    /// established upstream-tracking branch — what plain `git push` (no
    /// `-u`, no explicit refspec) needs to succeed at all, since a review
    /// worktree is expected to already have this set up by whatever
    /// created it (`GWorktreeAdd`/coordinator), not by `finalize_review_
    /// edits` itself. Returns `(work_dir, remote_dir, branch_name)`.
    fn init_repo_with_remote_tracking_branch(
        tag: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf, String) {
        let remote_dir = unique_temp_dir(&format!("{tag}-remote"));
        let work_dir = unique_temp_dir(&format!("{tag}-work"));
        git_in(&remote_dir, &["init", "--bare", "-q"]);
        git_in(&work_dir, &["init", "-q"]);
        git_in(&work_dir, &["config", "user.email", "t@t.com"]);
        git_in(&work_dir, &["config", "user.name", "T"]);
        std::fs::write(work_dir.join("f.txt"), "base\n").unwrap();
        git_in(&work_dir, &["add", "."]);
        git_in(&work_dir, &["commit", "-q", "-m", "base"]);
        git_in(
            &work_dir,
            &["remote", "add", "origin", remote_dir.to_str().unwrap()],
        );
        let branch = crate::core::git::current_branch(&work_dir).expect("branch after init");
        git_in(&work_dir, &["push", "-q", "-u", "origin", &branch]);
        (work_dir, remote_dir, branch)
    }

    /// Happy path: an edit made in the workspace while `review_target` is
    /// set gets committed and pushed to the real remote — asserted against
    /// the *pushed content at the remote*, not just "finalize returned
    /// Ok", so this would fail if finalize silently no-opped the push.
    #[test]
    fn finalize_review_edits_commits_and_pushes_to_origin() {
        let (work_dir, remote_dir, branch) =
            init_repo_with_remote_tracking_branch("finalize-happy");
        std::fs::write(work_dir.join("f.txt"), "base\nedited by reviewer\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(work_dir.clone());
        engine.review_target = Some(BranchReviewTarget {
            branch: branch.clone(),
            base: branch.clone(),
            host: None,
        });

        let summary = engine
            .finalize_review_edits("fix a typo during review")
            .expect("finalize should succeed against a real, reachable remote");
        assert!(
            summary.contains("Finalized"),
            "unexpected summary: {summary}"
        );

        let remote_content = crate::core::git::show_file_at_ref(&remote_dir, &branch, "f.txt")
            .expect("edited file must exist at the pushed remote ref");
        assert_eq!(
            remote_content, "base\nedited by reviewer\n",
            "the remote branch must carry the reviewer's edit, not just some push"
        );

        let _ = std::fs::remove_dir_all(&work_dir);
        let _ = std::fs::remove_dir_all(&remote_dir);
    }

    /// The user-facing surface for all of the above: `:GFinalize <message>`
    /// must actually reach [`Engine::finalize_review_edits`] with the
    /// typed message as the commit message (checked at the pushed remote,
    /// not just "some command ran") and report the result on `self.
    /// message`, the same status-line contract every other `:G*` command
    /// uses.
    #[test]
    fn gfinalize_command_commits_the_typed_message_and_pushes() {
        let (work_dir, remote_dir, branch) =
            init_repo_with_remote_tracking_branch("finalize-command");
        std::fs::write(work_dir.join("f.txt"), "base\nedited via command\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(work_dir.clone());
        engine.review_target = Some(BranchReviewTarget {
            branch: branch.clone(),
            base: branch.clone(),
            host: None,
        });

        let action = engine.execute_command("GFinalize typed commit message");
        assert_ne!(
            action,
            EngineAction::Error,
            "GFinalize must be a recognised command: {}",
            engine.message
        );
        assert!(
            engine.message.contains("Finalized"),
            "unexpected status message: {}",
            engine.message
        );

        let remote_head = crate::core::git::git_log(&remote_dir, 1);
        assert_eq!(
            remote_head.first().map(|e| e.message.as_str()),
            Some("typed commit message"),
            "the pushed remote commit must carry the message typed after :GFinalize"
        );

        let _ = std::fs::remove_dir_all(&work_dir);
        let _ = std::fs::remove_dir_all(&remote_dir);
    }

    /// Safety check: a review worktree that has since been checked out to
    /// a different branch than the one under review must refuse to
    /// finalize rather than silently pushing the human's edit onto
    /// whichever branch happens to be checked out — the provenance
    /// footgun #528 calls out. No commit/push may happen at all.
    #[test]
    fn finalize_review_edits_refuses_when_worktree_left_the_reviewed_branch() {
        let (work_dir, remote_dir, branch) =
            init_repo_with_remote_tracking_branch("finalize-wrong-branch");
        git_in(
            &work_dir,
            &["checkout", "-q", "-b", "not-the-reviewed-branch"],
        );
        std::fs::write(work_dir.join("f.txt"), "an edit made on the wrong branch\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(work_dir.clone());
        engine.review_target = Some(BranchReviewTarget {
            branch,
            base: "irrelevant".to_string(),
            host: None,
        });

        let err = engine
            .finalize_review_edits("should never land")
            .unwrap_err();
        assert!(
            err.contains("refusing to finalize"),
            "unexpected error: {err}"
        );
        // Nothing was committed: the edit is still an unstaged, uncommitted
        // change on disk.
        assert_eq!(
            crate::core::git::status_detailed(&work_dir).len(),
            1,
            "the edit must remain an uncommitted working-tree change"
        );

        let _ = std::fs::remove_dir_all(&work_dir);
        let _ = std::fs::remove_dir_all(&remote_dir);
    }

    /// A rejected (non-fast-forward) push must not lose the commit: the
    /// worktree keeps the real local commit for the human to resolve and
    /// retry, exactly the acceptance bar's "a failed push preserves the
    /// worktree and its commits."
    #[test]
    fn finalize_review_edits_failed_push_preserves_the_local_commit() {
        let (work_dir, remote_dir, branch) =
            init_repo_with_remote_tracking_branch("finalize-reject");

        // A second clone pushes first, moving `origin/<branch>` ahead of
        // what `work_dir` knows about — the next `work_dir` push is a
        // real, rejected non-fast-forward.
        let other_dir = unique_temp_dir("finalize-reject-other");
        let _ = std::fs::remove_dir_all(&other_dir);
        git_in(
            remote_dir.parent().unwrap(),
            &[
                "clone",
                "-q",
                remote_dir.to_str().unwrap(),
                other_dir.to_str().unwrap(),
            ],
        );
        git_in(&other_dir, &["config", "user.email", "t@t.com"]);
        git_in(&other_dir, &["config", "user.name", "T"]);
        std::fs::write(other_dir.join("f.txt"), "raced ahead\n").unwrap();
        git_in(&other_dir, &["commit", "-q", "-am", "raced ahead"]);
        git_in(&other_dir, &["push", "-q"]);

        std::fs::write(work_dir.join("f.txt"), "base\nlocal edit\n").unwrap();
        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(work_dir.clone());
        engine.review_target = Some(BranchReviewTarget {
            branch: branch.clone(),
            base: branch,
            host: None,
        });

        let err = engine
            .finalize_review_edits("local edit during review")
            .expect_err("push must be rejected as non-fast-forward");
        assert!(
            err.contains("push failed") && err.contains("preserved"),
            "unexpected error: {err}"
        );
        // The commit itself must still exist locally — finalize does not
        // roll it back just because the push failed.
        let log = crate::core::git::git_log(&work_dir, 5);
        assert!(
            log.iter()
                .any(|e| e.message.contains("local edit during review")),
            "the local commit must survive a rejected push, got log: {log:?}"
        );
        assert!(
            crate::core::git::status_detailed(&work_dir).is_empty(),
            "the edit was committed locally even though the push failed"
        );

        let _ = std::fs::remove_dir_all(&work_dir);
        let _ = std::fs::remove_dir_all(&remote_dir);
        let _ = std::fs::remove_dir_all(&other_dir);
    }

    #[test]
    fn reject_marks_the_entry_and_leaves_no_buffer_write() {
        let dir = unique_temp_dir("reject");
        let path = dir.join("reject_me.txt");
        std::fs::write(&path, "original\n").unwrap();
        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.open_change_review(vec![change(
            path.to_str().unwrap(),
            Some("original\n"),
            "changed\n",
        )]);
        engine.handle_change_review_key("", Some('r'));
        // Rejecting the only entry auto-closes the surface.
        assert!(engine.change_review.is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn accept_writes_new_text_to_disk_and_closes_when_all_decided() {
        let dir = unique_temp_dir("accept");
        let path = dir.join("accept_me.txt");
        std::fs::write(&path, "original\n").unwrap();
        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.open_change_review(vec![change(
            path.to_str().unwrap(),
            Some("original\n"),
            "changed\n",
        )]);
        engine.handle_change_review_key("", Some('a'));
        assert!(
            engine.change_review.is_none(),
            "surface auto-closes once the only entry is decided"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "changed\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// "Clicking a location jumps to that file and line" (#955's
    /// acceptance bar), exercised via `Return`'s keyboard-reachable
    /// equivalent (see this module's top doc for why the mouse path
    /// itself isn't wired yet): opens the buffer for the entry's path and
    /// lands the cursor on the changed row's 1-based line, converted to
    /// the engine's 0-based cursor line.
    #[test]
    fn return_jumps_to_the_current_rows_file_and_line() {
        let dir = unique_temp_dir("jump");
        let path = dir.join("jump_me.txt");
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.open_change_review(vec![change(
            path.to_str().unwrap(),
            Some("one\ntwo\nthree\n"),
            "one\nTWO\nthree\n",
        )]);
        // Scroll to row 1 — the changed "two"/"TWO" row.
        engine
            .change_review
            .as_mut()
            .unwrap()
            .current_entry_mut()
            .unwrap()
            .view
            .scroll_offset = 1;
        engine.handle_change_review_key("Return", None);

        assert!(
            engine.change_review.is_none(),
            "jumping closes the review surface"
        );
        let buf = engine.active_buffer_state();
        assert_eq!(buf.file_path.as_deref(), Some(path.as_path()));
        assert_eq!(
            engine.view().cursor.line,
            1,
            "0-based line for 1-based row 2"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn row_to_location_maps_a_changed_row_to_its_right_side_line() {
        let entry = crate::core::review::ChangeReviewState::new(vec![change(
            "f.rs",
            Some("one\ntwo\nthree\n"),
            "one\nTWO\nthree\n",
        )]);
        let entry = &entry.entries[0];
        // flat_rows: [one(Same), two/TWO(Changed), three(Same)] — row 1 is
        // the changed row, right_start = 1, so its 1-based right line is 2.
        let (path, line) = row_to_location(entry, 1).expect("row 1 is a real row");
        assert_eq!(path, "f.rs");
        assert_eq!(line, 2);
    }

    // ── verdict keys (#526) ──────────────────────────────────────────────

    fn install_mock_provider_with_approve_verdict(engine: &mut Engine) {
        use crate::core::extensions::{BoardProviderConfig, ExtensionManifest};
        let mut manifest = ExtensionManifest {
            name: "mock-board".to_string(),
            ..Default::default()
        };
        let mut verdict_commands = std::collections::HashMap::new();
        verdict_commands.insert(
            "approve".to_string(),
            vec![
                "mock".to_string(),
                "verdict".to_string(),
                "{id}".to_string(),
            ],
        );
        manifest.board = Some(BoardProviderConfig {
            refresh_command: vec!["mock-provider".to_string()],
            poll_interval_secs: 30,
            verdict_commands,
            ..Default::default()
        });
        engine
            .extension_state
            .installed
            .push(crate::core::session::InstalledExtension {
                name: manifest.name.clone(),
                version: String::new(),
            });
        engine.ext_registry = Some(vec![manifest]);
    }

    /// `A` while the surface is open closes it (same as Esc) and opens a
    /// verdict-composer buffer bound to the reviewed card — the
    /// keyboard-reachable path into #526's verdict flow.
    #[test]
    fn shift_a_closes_the_surface_and_opens_a_verdict_composer() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_approve_verdict(&mut engine);
        engine.open_change_review(vec![change("f.rs", Some("a"), "b")]);
        engine.review_card_id = Some("card:9".to_string());

        assert!(engine.handle_change_review_key("", Some('A')));

        assert!(
            engine.change_review.is_none(),
            "A must close the diff surface, same as Esc"
        );
        let binding = engine
            .active_buffer_state()
            .review_verdict
            .as_ref()
            .expect("A must open a verdict composer buffer");
        assert_eq!(binding.card_id, "card:9");
    }

    /// With no reviewed card behind the surface (an ACP tool-call diff,
    /// which `open_change_review` never binds a `review_card_id` for), `A`
    /// degrades to a status message rather than opening a composer for a
    /// card that doesn't exist.
    #[test]
    fn shift_a_with_no_reviewed_card_leaves_the_surface_open_with_a_message() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_approve_verdict(&mut engine);
        engine.open_change_review(vec![change("f.rs", Some("a"), "b")]);
        assert!(engine.review_card_id.is_none());

        assert!(engine.handle_change_review_key("", Some('A')));

        assert!(
            engine.change_review.is_some(),
            "no card to report against — the diff surface must stay open"
        );
        assert!(engine.message.contains("no card to report"));
    }
}
