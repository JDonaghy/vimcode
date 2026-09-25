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
use crate::core::review::ProposedChange;
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
    /// no fetch, no remote/ssh handling (that's #530).
    pub fn open_branch_review(&mut self, target: BranchReviewTarget) -> Result<(), String> {
        let root = self
            .workspace_root
            .clone()
            .ok_or_else(|| "no workspace open".to_string())?;
        let paths = crate::core::git::changed_files_between(&root, &target.base, &target.branch);
        if paths.is_empty() {
            return Err(format!(
                "no changes between '{}' and '{}'",
                target.base, target.branch
            ));
        }
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

/// Derive a 1-based `(path, line)` for `row_idx` (an index into
/// `entry.view.flat_rows()`) from the hunk that contains it — the same
/// per-row arithmetic `quadraui::unified_hunk_header` uses per-hunk, just
/// walked row by row. Prefers the right-side (new) line number, falling
/// back to the left-side one for a pure-removal row that has no right
/// side at all.
fn row_to_location(
    entry: &crate::core::review::ChangeReviewEntry,
    row_idx: usize,
) -> Option<(String, u32)> {
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
                base,
            })
            .expect("branch review should build a change list");

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
            })
            .unwrap_err();
        assert!(err.contains("no workspace"));
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
}
