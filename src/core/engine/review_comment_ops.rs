//! Pinned review comments (#527, Track A Phase 3): anchor a short note to
//! whichever line the open change-review surface's `DiffView` is
//! currently scrolled to (the same row `Return`/`change_review_jump_to_hit`
//! resolves — see `crate::core::review::row_to_location`'s doc), edit or
//! delete it before a verdict is reported, and fold the collected list
//! into the verdict body #526 composes.
//!
//! ## Why a `Dialog`, not a new overlay
//!
//! A pinned comment is one line of prose. `Dialog`'s existing `input:
//! Option<DialogInput>` field (already used for the SSH passphrase prompt,
//! `sc_show_passphrase_dialog`, and the "move file to…" prompt) is the
//! established in-canvas single-line text-entry primitive on both
//! backends — reusing it here means no new keyboard-routing tier, no new
//! rendering, and it composes for free with the change-review surface
//! already being open underneath: `Engine::handle_key_pressed` checks
//! `self.dialog.is_some()` *before* `self.change_review.is_some()`
//! (`keys.rs`), so opening this dialog doesn't close the diff surface —
//! it just borrows the keyboard for one line of text and hands it back.

use super::*;
use crate::core::review::ReviewComment;

/// Which pinned comment a still-open `Dialog` (tag `"review_comment"`) is
/// editing. Set by [`Engine::change_review_start_comment`] right before
/// the dialog opens, read and cleared by
/// [`Engine::apply_review_comment_dialog_result`] once the dialog is
/// dismissed — a `Dialog` itself carries no room for extra payload beyond
/// its tag/title/body/buttons/input, so this is where that payload lives
/// instead.
#[derive(Debug, Clone)]
pub struct ReviewCommentTarget {
    pub file: String,
    pub line: u32,
    /// `Some(i)`: overwrite `comments[i]`'s text (the dialog was opened on
    /// a line that already had a pin). `None`: append a new
    /// [`ReviewComment`] instead.
    pub existing_index: Option<usize>,
}

impl Engine {
    /// `c` in the change-review surface: open a one-line text dialog to
    /// pin a new comment on the current line, or edit the one already
    /// there (prefilling the dialog's input with its current text). A
    /// no-op (status message only) if no review is open or the review has
    /// no current row to anchor to (an empty review, in practice
    /// unreachable since [`Engine::open_change_review`] refuses an empty
    /// change list).
    pub(crate) fn change_review_start_comment(&mut self) {
        let Some(review) = self.change_review.as_ref() else {
            return;
        };
        let Some((file, line)) = review.current_location() else {
            self.message = "Board: no line to comment on".to_string();
            return;
        };
        let existing_index = review.comment_index_at(&file, line);
        let initial_text = existing_index
            .and_then(|i| review.comments.get(i))
            .map(|c| c.text.clone())
            .unwrap_or_default();
        self.review_comment_target = Some(ReviewCommentTarget {
            file: file.clone(),
            line,
            existing_index,
        });
        let verb = if existing_index.is_some() {
            "Edit"
        } else {
            "Add"
        };
        let mut dialog = Dialog {
            tag: "review_comment".to_string(),
            title: format!("{verb} comment — {file}:{line}"),
            body: Vec::new(),
            buttons: vec![
                DialogButton {
                    label: "Cancel".into(),
                    hotkey: '\0',
                    action: "cancel".into(),
                },
                DialogButton {
                    label: "Save".into(),
                    hotkey: '\0',
                    action: "save".into(),
                },
            ],
            selected: 1,
            input: Some(DialogInput {
                label: "Comment".into(),
                value: initial_text,
                is_password: false,
            }),
        };
        // Suppress hotkeys — an input dialog uses printable chars for
        // typing, same precedent `sc_show_passphrase_dialog` set.
        dialog.buttons[0].hotkey = '\0';
        dialog.buttons[1].hotkey = '\0';
        self.dialog = Some(dialog);
    }

    /// `d` in the change-review surface: delete the pinned comment on the
    /// current line, if any — a single keypress with no confirmation
    /// dialog, mirroring `r` (reject) already being exactly that. A no-op
    /// (status message only) when there is nothing pinned to the current
    /// line.
    pub(crate) fn change_review_delete_comment_at_current_line(&mut self) {
        let Some(review) = self.change_review.as_mut() else {
            return;
        };
        let Some((file, line)) = review.current_location() else {
            return;
        };
        let Some(idx) = review.comment_index_at(&file, line) else {
            self.message = "Board: no comment on this line".to_string();
            return;
        };
        review.delete_comment(idx);
        self.message = "Comment deleted".to_string();
    }

    /// Resolve the `"review_comment"` dialog's result (called from
    /// `Engine::process_dialog_result`): `action == "save"` commits the
    /// pending add/edit using [`Self::review_comment_target`] (taken —
    /// always cleared, whichever way this resolves); any other action
    /// (`"cancel"`/Escape) discards it. Saving an empty/whitespace-only
    /// body deletes an in-progress edit rather than leaving a blank pin
    /// (and simply discards a new one), so clearing the text field is a
    /// legitimate way to remove a comment without a separate `d` keypress.
    pub(crate) fn apply_review_comment_dialog_result(
        &mut self,
        action: &str,
        input_value: Option<&str>,
    ) {
        let Some(target) = self.review_comment_target.take() else {
            return;
        };
        if action != "save" {
            return;
        }
        let text = input_value.unwrap_or("").trim().to_string();
        let Some(review) = self.change_review.as_mut() else {
            return;
        };
        if text.is_empty() {
            if let Some(idx) = target.existing_index {
                review.delete_comment(idx);
                self.message = "Comment deleted (left empty)".to_string();
            } else {
                self.message = "Comment discarded: empty text".to_string();
            }
            return;
        }
        if let Some(idx) = target.existing_index {
            review.edit_comment(idx, text);
            self.message = "Comment updated".to_string();
        } else {
            review.comments.push(ReviewComment {
                file: target.file,
                line: target.line,
                text,
            });
            self.message = "Comment added".to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::review::{ChangeReviewState, ProposedChange};

    fn open_review(engine: &mut Engine, path: &str, old: &str, new: &str) {
        engine.change_review = Some(ChangeReviewState::new(vec![ProposedChange {
            path: path.to_string(),
            old_text: Some(old.to_string()),
            new_text: new.to_string(),
        }]));
    }

    #[test]
    fn start_comment_with_no_review_open_is_a_noop_not_a_panic() {
        let mut engine = Engine::new_for_test();
        engine.change_review_start_comment();
        assert!(engine.dialog.is_none());
    }

    #[test]
    fn start_comment_opens_a_dialog_bound_to_the_current_line() {
        let mut engine = Engine::new_for_test();
        open_review(&mut engine, "f.rs", "old\n", "new\n");

        engine.change_review_start_comment();

        let dialog = engine.dialog.as_ref().expect("dialog must open");
        assert_eq!(dialog.tag, "review_comment");
        assert!(dialog.input.is_some());
        assert_eq!(dialog.input.as_ref().unwrap().value, "");
        let target = engine
            .review_comment_target
            .as_ref()
            .expect("target must be recorded");
        assert_eq!(target.file, "f.rs");
        assert_eq!(target.line, 1);
        assert!(target.existing_index.is_none());
    }

    #[test]
    fn start_comment_on_an_already_commented_line_prefills_the_edit() {
        let mut engine = Engine::new_for_test();
        open_review(&mut engine, "f.rs", "old\n", "new\n");
        engine
            .change_review
            .as_mut()
            .unwrap()
            .comments
            .push(ReviewComment {
                file: "f.rs".to_string(),
                line: 1,
                text: "already here".to_string(),
            });

        engine.change_review_start_comment();

        let dialog = engine.dialog.as_ref().expect("dialog must open");
        assert_eq!(dialog.title, "Edit comment — f.rs:1");
        assert_eq!(dialog.input.as_ref().unwrap().value, "already here");
        assert_eq!(
            engine
                .review_comment_target
                .as_ref()
                .unwrap()
                .existing_index,
            Some(0)
        );
    }

    /// Full round-trip through the same path a keypress would take:
    /// `c` opens the dialog, typing + `Return` (simulated here by calling
    /// the dialog-result handler directly, the same call
    /// `process_dialog_result`'s `"review_comment"` arm makes) pins the
    /// comment onto the review.
    #[test]
    fn save_dialog_result_adds_a_new_comment() {
        let mut engine = Engine::new_for_test();
        open_review(&mut engine, "f.rs", "old\n", "new\n");
        engine.change_review_start_comment();

        engine.apply_review_comment_dialog_result("save", Some("needs a null check"));

        assert!(
            engine.review_comment_target.is_none(),
            "the target must be cleared once resolved"
        );
        let review = engine.change_review.as_ref().unwrap();
        assert_eq!(review.comments.len(), 1);
        assert_eq!(review.comments[0].file, "f.rs");
        assert_eq!(review.comments[0].line, 1);
        assert_eq!(review.comments[0].text, "needs a null check");
    }

    #[test]
    fn save_dialog_result_edits_an_existing_comment_in_place() {
        let mut engine = Engine::new_for_test();
        open_review(&mut engine, "f.rs", "old\n", "new\n");
        engine
            .change_review
            .as_mut()
            .unwrap()
            .comments
            .push(ReviewComment {
                file: "f.rs".to_string(),
                line: 1,
                text: "before".to_string(),
            });
        engine.change_review_start_comment();

        engine.apply_review_comment_dialog_result("save", Some("after"));

        let review = engine.change_review.as_ref().unwrap();
        assert_eq!(review.comments.len(), 1, "must edit, not duplicate");
        assert_eq!(review.comments[0].text, "after");
    }

    #[test]
    fn cancel_dialog_result_leaves_comments_untouched() {
        let mut engine = Engine::new_for_test();
        open_review(&mut engine, "f.rs", "old\n", "new\n");
        engine.change_review_start_comment();

        engine.apply_review_comment_dialog_result("cancel", Some("typed but abandoned"));

        assert!(engine.review_comment_target.is_none());
        assert!(engine.change_review.as_ref().unwrap().comments.is_empty());
    }

    #[test]
    fn saving_empty_text_discards_a_new_comment() {
        let mut engine = Engine::new_for_test();
        open_review(&mut engine, "f.rs", "old\n", "new\n");
        engine.change_review_start_comment();

        engine.apply_review_comment_dialog_result("save", Some("   "));

        assert!(engine.change_review.as_ref().unwrap().comments.is_empty());
    }

    #[test]
    fn saving_empty_text_over_an_existing_comment_deletes_it() {
        let mut engine = Engine::new_for_test();
        open_review(&mut engine, "f.rs", "old\n", "new\n");
        engine
            .change_review
            .as_mut()
            .unwrap()
            .comments
            .push(ReviewComment {
                file: "f.rs".to_string(),
                line: 1,
                text: "will be cleared".to_string(),
            });
        engine.change_review_start_comment();

        engine.apply_review_comment_dialog_result("save", Some(""));

        assert!(engine.change_review.as_ref().unwrap().comments.is_empty());
    }

    #[test]
    fn delete_at_current_line_removes_only_that_comment() {
        let mut engine = Engine::new_for_test();
        open_review(&mut engine, "f.rs", "old\n", "new\n");
        {
            let review = engine.change_review.as_mut().unwrap();
            review.comments.push(ReviewComment {
                file: "f.rs".to_string(),
                line: 1,
                text: "on the current line".to_string(),
            });
            review.comments.push(ReviewComment {
                file: "f.rs".to_string(),
                line: 99,
                text: "elsewhere".to_string(),
            });
        }

        engine.change_review_delete_comment_at_current_line();

        let review = engine.change_review.as_ref().unwrap();
        assert_eq!(review.comments.len(), 1);
        assert_eq!(review.comments[0].line, 99);
        assert_eq!(engine.message, "Comment deleted");
    }

    #[test]
    fn delete_at_current_line_with_nothing_pinned_is_a_noop_message() {
        let mut engine = Engine::new_for_test();
        open_review(&mut engine, "f.rs", "old\n", "new\n");

        engine.change_review_delete_comment_at_current_line();

        assert!(engine.message.contains("no comment on this line"));
    }

    /// `c`/`d` routed all the way through `handle_change_review_key`, the
    /// real keyboard entry point — not just the `Engine::change_review_*`
    /// methods directly, so this covers the actual key-dispatch wiring
    /// too.
    #[test]
    fn c_and_d_keys_route_through_handle_change_review_key() {
        let mut engine = Engine::new_for_test();
        open_review(&mut engine, "f.rs", "old\n", "new\n");

        assert!(engine.handle_change_review_key("", Some('c')));
        assert!(engine.dialog.is_some(), "'c' must open the comment dialog");
        engine.apply_review_comment_dialog_result("save", Some("pinned via key"));
        assert_eq!(engine.change_review.as_ref().unwrap().comments.len(), 1);

        assert!(engine.handle_change_review_key("", Some('d')));
        assert!(
            engine.change_review.as_ref().unwrap().comments.is_empty(),
            "'d' must delete the comment on the current line"
        );
    }
}
