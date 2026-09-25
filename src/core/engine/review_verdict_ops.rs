//! Review verdict reporting (#526, Track A Phase 2): close the loop on the
//! change-review surface (#525/#955) by approving or requesting changes,
//! routed to a provider-declared command through a temp file (never
//! inline — see `crate::core::tool_client::write_review_body_temp_file`'s
//! doc for why).
//!
//! This is the generic host's `core` half, same placement rule as
//! `board_ops.rs`/`document_ops.rs`: it knows how to find a configured
//! provider (an installed extension whose manifest declares `[board]` with
//! `verdict_commands`, #522), compose a review body as a real markdown
//! scratch buffer (mirroring `document_ops.rs`'s provider-document buffer
//! exactly — same "capture the provider's argv at open time, push on `:w`"
//! shape), and run its verdict command through the same `board_client`
//! seam `board_ops.rs` already uses. It names **no specific external
//! provider or verdict vocabulary beyond `crate::core::review::
//! ReviewVerdict`'s own generic approve/request-changes/comment terms** —
//! a dedicated repo-root test enforces this mechanically.
//!
//! ## Why a scratch buffer, not an inline prompt
//!
//! A review body is prose (and, once #527 lands, collected inline
//! comments) — multi-line, possibly containing code fences. `document_ops.
//! rs` already proved the "open a real markdown buffer, `:w` pushes it
//! through a provider command" shape works well for exactly this kind of
//! payload; reusing it here means the same vim editing (undo, search,
//! paste from other buffers) is available for composing a review as it is
//! for authoring an issue.
//!
//! ## A failed verdict command preserves the composed body (#526's own
//! acceptance bar)
//!
//! [`Engine::save_review_verdict_buffer`] never clears the buffer or its
//! `dirty` flag on failure — the composed text stays exactly where the
//! user left it (still in the buffer, still on disk in the temp file the
//! failed attempt wrote), so a rejected/erroring provider command never
//! costs a retype-from-scratch — a real failure mode a pipeline-management
//! bundle's own verdict-reporting channel has been bitten by before.

use super::*;
use crate::core::buffer_manager::ReviewVerdictBinding;
use crate::core::review::ReviewVerdict;

impl Engine {
    /// Begin composing a verdict for the currently (or most recently) open
    /// change-review surface's reviewed card: closes the diff surface (same
    /// effect as Esc) and opens a markdown scratch buffer bound to
    /// `verdict` and the reviewed card's id; `:w`
    /// ([`Self::save_review_verdict_buffer`]) reports it.
    ///
    /// A no-op (status message only, never a panic) if: this review has no
    /// card behind it (`review_card_id` is `None` — an ACP tool-call diff,
    /// say, has no verdict to report), no board provider is configured, or
    /// the provider declared no command for `verdict` at all — the same
    /// "missing/failing provider command degrades to a message" precedent
    /// `board_ops.rs`'s `open_review_card` already set.
    pub(crate) fn start_review_verdict(&mut self, verdict: ReviewVerdict) {
        let Some(card_id) = self.review_card_id.clone() else {
            self.message = "Board: this review has no card to report a verdict on".to_string();
            return;
        };
        let Some(provider) = self.board_provider() else {
            self.message = "Board: no provider configured".to_string();
            return;
        };
        let Some(verdict_command) = provider.verdict_commands.get(verdict.token()).cloned() else {
            self.message = format!("Board: no {} command configured", verdict.token());
            return;
        };
        if verdict_command.is_empty() {
            self.message = format!("Board: no {} command configured", verdict.token());
            return;
        }
        self.close_change_review();
        self.open_review_verdict_buffer(card_id, verdict, verdict_command);
    }

    fn open_review_verdict_buffer(
        &mut self,
        card_id: String,
        verdict: ReviewVerdict,
        verdict_command: Vec<String>,
    ) {
        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str("");
            state.syntax = crate::core::syntax::Syntax::new_from_path(Some("review.md"));
            state.review_verdict = Some(ReviewVerdictBinding {
                card_id: card_id.clone(),
                verdict_command,
            });
            state.dirty = false;
            state.scratch_name = Some(format!("[Review {}: {}]", verdict.token(), card_id));
        }

        // Open in a new tab (same pattern as `open_tool_document_buffer`).
        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        self.message = "Write the review body, then :w to report it.".to_string();
    }

    /// Save a review-verdict buffer: write the composed body to a temp file
    /// and run the bound provider's verdict command with `{id}`/
    /// `{body_file}` substituted. **Blocking**, the same deliberate
    /// tradeoff `save_tool_document_buffer` already made.
    ///
    /// The temp file is written *before* the command runs, so a command
    /// failure never loses the body: it is still on disk at the path just
    /// written, and — since this function returns an `Err` without
    /// touching the buffer's content or `dirty` flag — still exactly as
    /// the user left it in the open buffer too (#526's "preserve the
    /// composed body on failure" acceptance bar).
    pub(crate) fn save_review_verdict_buffer(&mut self) -> Result<(), String> {
        let binding = self
            .active_buffer_state()
            .review_verdict
            .clone()
            .ok_or_else(|| "not a review verdict buffer".to_string())?;
        let body = self.buffer().to_string();

        let body_file = crate::core::tool_client::write_review_body_temp_file(&body)
            .map_err(|e| format!("could not write the review body to a temp file: {e}"))?;
        let body_file = body_file.to_string_lossy().to_string();
        let argv: Vec<String> = binding
            .verdict_command
            .iter()
            .map(|a| {
                a.replace("{id}", &binding.card_id)
                    .replace("{body_file}", &body_file)
            })
            .collect();

        self.board_client.run(&argv).map_err(|e| e.user_message())?;

        self.active_buffer_state_mut().dirty = false;
        self.message = "Verdict reported".to_string();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::extensions::{BoardProviderConfig, ExtensionManifest};
    use crate::core::tool_client::{MockToolClient, RecordingToolClient, ToolError};

    fn install_mock_provider_with_verdicts(engine: &mut Engine) {
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
                "--ok".to_string(),
                "--body-file".to_string(),
                "{body_file}".to_string(),
            ],
        );
        verdict_commands.insert(
            "request-changes".to_string(),
            vec![
                "mock".to_string(),
                "verdict".to_string(),
                "{id}".to_string(),
                "--changes".to_string(),
                "--body-file".to_string(),
                "{body_file}".to_string(),
            ],
        );
        manifest.board = Some(BoardProviderConfig {
            refresh_command: vec!["mock-provider".to_string()],
            poll_interval_secs: 30,
            actions: Default::default(),
            verdict_commands,
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

    #[test]
    fn start_review_verdict_with_no_reviewed_card_is_a_noop_not_a_panic() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_verdicts(&mut engine);
        engine.review_card_id = None;
        let tabs_before = engine.active_group().tabs.len();
        engine.start_review_verdict(ReviewVerdict::Approve);
        assert_eq!(engine.active_group().tabs.len(), tabs_before);
        assert!(engine.message.contains("no card to report"));
    }

    #[test]
    fn start_review_verdict_with_no_provider_is_a_noop_not_a_panic() {
        let mut engine = Engine::new_for_test();
        engine.review_card_id = Some("card:1".to_string());
        let tabs_before = engine.active_group().tabs.len();
        engine.start_review_verdict(ReviewVerdict::Approve);
        assert_eq!(engine.active_group().tabs.len(), tabs_before);
        assert!(engine.message.contains("no provider configured"));
    }

    #[test]
    fn start_review_verdict_with_no_command_configured_for_that_verdict_is_a_noop() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_verdicts(&mut engine);
        engine.review_card_id = Some("card:1".to_string());
        // "comment" has no entry in `install_mock_provider_with_verdicts`.
        engine.start_review_verdict(ReviewVerdict::Comment);
        assert!(engine.message.contains("no comment command configured"));
        assert!(engine.active_buffer_state().review_verdict.is_none());
    }

    #[test]
    fn start_review_verdict_opens_a_composer_buffer_bound_to_the_card_and_verdict() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_verdicts(&mut engine);
        engine.review_card_id = Some("card:7".to_string());

        engine.start_review_verdict(ReviewVerdict::Approve);

        let binding = engine
            .active_buffer_state()
            .review_verdict
            .as_ref()
            .expect("opening a verdict composer must bind the new buffer");
        assert_eq!(binding.card_id, "card:7");
        assert_eq!(binding.verdict_command[0], "mock");
        assert_eq!(binding.verdict_command[3], "--ok");
    }

    /// Acceptance: "Approve / request-changes records a verdict through a
    /// mock provider, with no coordinator present" (#526) — and the body,
    /// including newlines, arrives intact via `{body_file}`.
    #[test]
    fn save_review_verdict_buffer_reports_through_the_provider_with_the_body_file() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_verdicts(&mut engine);
        engine.review_card_id = Some("card:7".to_string());
        engine.start_review_verdict(ReviewVerdict::RequestChanges);

        let body = "Looks mostly good.\n\n```rust\nfn f() {}\n```\n\nOne nit on line 12.\n";
        engine.active_buffer_state_mut().buffer.content = ropey::Rope::from_str(body);

        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());
        engine
            .save()
            .expect(":w should report the verdict through the mock provider");

        let calls = recorder.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].argv[0], "mock");
        assert_eq!(
            calls[0].argv[2], "card:7",
            "{{id}} must substitute the card id"
        );
        assert_eq!(calls[0].argv[3], "--changes");
        let body_file_path = &calls[0].argv[5];
        assert_ne!(
            body_file_path, "{body_file}",
            "{{body_file}} must be substituted, not left literal"
        );
        let written = std::fs::read_to_string(body_file_path)
            .expect("the body file argv points at must actually exist on disk");
        assert_eq!(
            written, body,
            "the body file must carry the composed text byte-for-byte, \
             newlines and code fences included"
        );
        let _ = std::fs::remove_file(body_file_path);

        assert_eq!(engine.message, "Verdict reported");
        assert!(!engine.active_buffer_state().dirty);
    }

    /// #526's own acceptance bar: "a failed verdict command preserves the
    /// composed body." The buffer's content and dirty flag are untouched
    /// on failure — no retype-from-scratch.
    #[test]
    fn save_review_verdict_buffer_preserves_the_body_on_a_failing_provider() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_verdicts(&mut engine);
        engine.review_card_id = Some("card:7".to_string());
        engine.start_review_verdict(ReviewVerdict::Approve);

        let body = "Detailed findings that took a while to write.\n";
        engine.active_buffer_state_mut().buffer.content = ropey::Rope::from_str(body);
        engine.active_buffer_state_mut().dirty = true;

        engine.set_board_client_for_test(MockToolClient(Err(ToolError::NonZeroExit {
            code: Some(1),
            stderr: "rejected: needs a summary line".to_string(),
        })));

        let err = engine.save().unwrap_err();
        assert!(err.contains("rejected") || err.contains("status 1"));

        assert_eq!(
            engine.buffer().to_string(),
            body,
            "the composed body must still be exactly in the buffer after a failed report"
        );
        assert!(
            engine.active_buffer_state().dirty,
            "a failed report must not silently mark the buffer clean"
        );
        assert_ne!(engine.message, "Verdict reported");
    }

    #[test]
    fn ordinary_buffer_save_is_unaffected_by_review_verdict_wiring() {
        let mut engine = Engine::new_for_test();
        assert!(engine.active_buffer_state().review_verdict.is_none());
        let err = engine.save().unwrap_err();
        assert_eq!(err, "No file name");
    }
}
