//! Board panel engine plumbing (#521 — Phase 0, read-only).
//!
//! This module is the generic host's `core` half: it knows how to find a
//! configured provider (an installed extension whose manifest declares
//! `[board]`, #522), run it through the [`crate::core::tool_client`] seam,
//! and translate `quadraui::BoardAction`s into mutations on the cached
//! `quadraui::BoardModel`. It names **no specific external provider** — see
//! `tool_client`'s module doc for the placement rule this satisfies, which a
//! dedicated repo-root test enforces mechanically.
//!
//! Phase 0 is read-only: only `SelectCard` and `OpenIssue` are handled below
//! (selection + a status-line acknowledgement, or — when a `[document]`
//! provider is configured, #524 — opening the card as an editable markdown
//! buffer via `Engine::open_tool_document`). Actions that would mutate a
//! provider's state (`Dispatch`, `RecordTest`, `Merge`, …) are #523's scope.

use super::*;
use crate::core::extensions::BoardProviderConfig;
use crate::core::tool_client::{self, ToolError};

impl Engine {
    /// The first installed extension that declares a `[board]` provider, if
    /// any. "First" rather than "the only one" — nothing here assumes a
    /// single extension can provide a board, it just doesn't yet pick among
    /// several (no reported need for that in Phase 0).
    pub fn board_provider(&self) -> Option<BoardProviderConfig> {
        self.ext_installed_manifests()
            .into_iter()
            .find_map(|m| m.board)
    }

    /// Swap in a mock [`ToolClient`] for tests — every consumer of the
    /// Board panel must be exercisable with no provider binary installed
    /// anywhere.
    #[cfg(test)]
    pub fn set_board_client_for_test(
        &mut self,
        client: impl crate::core::tool_client::ToolClient + 'static,
    ) {
        self.board_client = std::sync::Arc::new(client);
    }

    /// Spawn a background refresh against the configured provider's
    /// `refresh_command`. No-op if a refresh is already in flight, no
    /// provider is configured, or the provider declared an empty argv.
    /// Result arrives via [`Self::poll_board`].
    pub fn board_refresh(&mut self) {
        if self.board_fetching {
            return;
        }
        let Some(provider) = self.board_provider() else {
            return;
        };
        if provider.refresh_command.is_empty() {
            return;
        }
        let client = std::sync::Arc::clone(&self.board_client);
        let argv = provider.refresh_command;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = tool_client::fetch_board_model(client.as_ref(), &argv);
            let _ = tx.send(result);
        });
        self.board_rx = Some(rx);
        self.board_fetching = true;
        self.board_last_refresh = Some(std::time::Instant::now());
    }

    /// Non-blocking check for a completed refresh. Call from `poll_idle`.
    pub fn poll_board(&mut self) -> bool {
        let result: Option<Result<quadraui::BoardModel, ToolError>> =
            self.board_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        let Some(result) = result else {
            return false;
        };
        self.board_fetching = false;
        self.board_rx = None;
        match result {
            Ok(model) => {
                self.board_model = Some(model);
                self.board_error = None;
            }
            Err(err) => {
                self.board_error = Some(err.user_message());
            }
        }
        true
    }

    /// Refresh on the provider's declared `poll_interval_secs` cadence,
    /// while the Board panel is the active sidebar panel. Call once per
    /// `poll_idle` tick. A provider with no `board_last_refresh` yet
    /// (never fetched) is always due.
    pub(crate) fn tick_board(&mut self) {
        if !self.active_panel_is(sidebar::PANEL_BOARD) {
            return;
        }
        if self.board_fetching {
            return;
        }
        let Some(provider) = self.board_provider() else {
            return;
        };
        let interval = std::time::Duration::from_secs(provider.poll_interval_secs.max(1));
        let due = self
            .board_last_refresh
            .is_none_or(|t| t.elapsed() >= interval);
        if due {
            self.board_refresh();
        }
    }

    /// Apply a semantic [`quadraui::BoardAction`] to the cached model.
    /// Phase 0 only handles the read-only actions (`SelectCard`,
    /// `MoveSelection`, `JumpToTop`/`JumpToBottom`, `OpenIssue`,
    /// `OpenReview` — #525); every other variant is a provider-dispatch
    /// action out of scope until #523 and is a deliberate no-op here.
    pub fn apply_board_action(&mut self, action: quadraui::BoardAction) {
        use quadraui::BoardAction;
        // `OpenIssue`/`OpenReview` both need `&mut self` (to open a
        // document buffer or the change-review surface), which can't
        // happen while `model` still holds `self.board_model` borrowed —
        // resolve to an owned pending action first and handle it after the
        // match, once that borrow has ended.
        enum Pending {
            OpenIssue(quadraui::WidgetId),
            OpenReview(quadraui::WidgetId),
        }
        let pending = {
            let Some(model) = self.board_model.as_mut() else {
                return;
            };
            match action {
                BoardAction::SelectCard(id) => {
                    model.selected_card_id = Some(id);
                    None
                }
                BoardAction::MoveSelection(dir) => {
                    model.move_selection(dir);
                    None
                }
                BoardAction::JumpToTop => {
                    model.jump_to_top();
                    None
                }
                BoardAction::JumpToBottom => {
                    model.jump_to_bottom();
                    None
                }
                BoardAction::OpenIssue(id) => Some(Pending::OpenIssue(id)),
                BoardAction::OpenReview(id) => Some(Pending::OpenReview(id)),
                // Provider-dispatch actions (#523) and context menu —
                // no-op in this read-only phase.
                BoardAction::ContextMenu(..) => None,
            }
        };
        match pending {
            Some(Pending::OpenIssue(id)) => self.open_issue_card(id),
            Some(Pending::OpenReview(id)) => self.open_review_card(id),
            None => {}
        }
    }

    /// `OpenIssue`'s handling: if a document provider is configured
    /// (#524), open the card as an editable markdown buffer seeded by the
    /// provider's read command. Otherwise fall back to Phase 0's
    /// acknowledgement — echo the cached card's title to the status line —
    /// so a board with no document provider still gives feedback on open.
    fn open_issue_card(&mut self, id: quadraui::WidgetId) {
        if self.document_provider().is_some() {
            match self.open_tool_document(id.as_str()) {
                Ok(()) => {}
                Err(e) => self.message = format!("Board: {e}"),
            }
            return;
        }
        let title = self.board_model.as_ref().and_then(|model| {
            model
                .columns
                .iter()
                .flat_map(|c| c.cards.iter())
                .find(|c| c.id == id)
                .map(|c| c.title.clone())
        });
        if let Some(title) = title {
            self.message = format!("Board: {title}");
        }
    }

    /// `OpenReview`'s handling (#525): run the configured board provider's
    /// `"OpenReview"` action (`BoardProviderConfig::action_argv`) to
    /// resolve this card to a
    /// [`crate::core::tool_client::BranchReviewTarget`], then hand that
    /// off to `Engine::open_branch_review`, which turns it into a local
    /// git diff and opens the shared change-review surface (#955). This
    /// function is the only place that knows the review comes from a board
    /// card — `open_branch_review` itself has no idea.
    ///
    /// **Blocking**, the same one-shot tradeoff `open_issue_card`'s
    /// document-provider path already made: opening a review is a
    /// deliberate user action with no cached state to show while waiting.
    fn open_review_card(&mut self, id: quadraui::WidgetId) {
        let Some(provider) = self.board_provider() else {
            self.message = "Board: no provider configured".to_string();
            return;
        };
        let Some(argv) = provider.action_argv("OpenReview", id.as_str()) else {
            self.message = "Board: no review command configured".to_string();
            return;
        };
        let target =
            match tool_client::fetch_branch_review_target(self.board_client.as_ref(), &argv) {
                Ok(target) => target,
                Err(e) => {
                    self.message = format!("Board: {}", e.user_message());
                    return;
                }
            };
        if let Err(e) = self.open_branch_review(target) {
            self.message = format!("Board: {e}");
        }
    }

    /// Keyboard dispatch for the Board panel — the
    /// `dispatch_*_sidebar_key_unified` pattern every other panel uses.
    ///
    /// `Escape` leaves the panel (mirrors every sibling panel's exit key);
    /// `h`/`Left` and `l`/`Right` are left to `quadraui::BoardModel::
    /// handle_key`'s own generic keymap for column navigation instead,
    /// since (unlike the other panels) they are meaningful board
    /// navigation here, not an "exit to the activity bar" gesture. `R`
    /// (#525) opens the review for the selected card — `handle_key`'s own
    /// doc explicitly calls "review" out as a workflow-specific verb hosts
    /// should handle themselves, so it's dispatched here rather than added
    /// to quadraui's generic keymap.
    ///
    /// Returns whether the panel is still focused afterward.
    pub fn dispatch_board_key_unified(&mut self, key: &str) -> bool {
        if key == "Escape" {
            self.board_has_focus = false;
            return false;
        }
        if key == "R" {
            if let Some(id) = self
                .board_model
                .as_ref()
                .and_then(|m| m.selected_card_id.clone())
            {
                self.apply_board_action(quadraui::BoardAction::OpenReview(id));
            }
            return true;
        }
        // Both backends' `engine_key_from_ui`/`map_gtk_key_name` translate
        // an Enter keypress to the engine's own `"Return"`/`"KP_Enter"`
        // convention (see `keys.rs`), but `quadraui::BoardModel::handle_key`
        // matches the literal `"Enter"` — translate here (a real key never
        // reaches this function spelled `"Enter"`), so pressing Enter on a
        // selected card actually opens it instead of `handle_key` silently
        // returning `None` for a key it doesn't recognise.
        let key = if key == "Return" || key == "KP_Enter" {
            "Enter"
        } else {
            key
        };
        if let Some(action) = self
            .board_model
            .as_ref()
            .and_then(|m| m.handle_key(key, quadraui::Modifiers::default()))
        {
            self.apply_board_action(action);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::extensions::ExtensionManifest;
    use crate::core::tool_client::MockToolClient;
    use quadraui::{BadgeStatus, BoardAction, BoardCard, BoardColumn, BoardModel, WidgetId};

    fn card(id: &str, title: &str) -> BoardCard {
        BoardCard {
            id: WidgetId::new(id),
            title: title.to_string(),
            labels: vec![],
            badges: vec![quadraui::CardBadge {
                label: "P".into(),
                status: BadgeStatus::Passed,
            }],
            hint: None,
        }
    }

    fn fixture_model() -> BoardModel {
        BoardModel {
            id: WidgetId::new("board"),
            columns: vec![BoardColumn {
                id: WidgetId::new("col:backlog"),
                title: "Backlog".into(),
                cards: vec![card("card:1", "Example card"), card("card:2", "Another")],
                scroll_offset: 0,
            }],
            selected_card_id: Some(WidgetId::new("card:1")),
            col_scroll_offset: 0,
        }
    }

    fn install_mock_provider(engine: &mut Engine, model: BoardModel) {
        let mut manifest = ExtensionManifest {
            name: "mock-board".to_string(),
            ..Default::default()
        };
        manifest.board = Some(BoardProviderConfig {
            refresh_command: vec!["mock-provider".to_string()],
            poll_interval_secs: 30,
            actions: Default::default(),
        });
        engine
            .extension_state
            .installed
            .push(crate::core::session::InstalledExtension {
                name: manifest.name.clone(),
                version: String::new(),
            });
        engine.ext_registry = Some(vec![manifest]);
        let json = serde_json::to_value(&model).unwrap();
        engine.set_board_client_for_test(MockToolClient(Ok(json)));
    }

    #[test]
    fn no_provider_configured_is_not_an_error() {
        let engine = Engine::new_for_test();
        assert!(engine.board_provider().is_none());
        assert!(engine.board_model.is_none());
        assert!(engine.board_error.is_none());
    }

    #[test]
    fn board_refresh_with_mock_provider_populates_model() {
        let mut engine = Engine::new_for_test();
        install_mock_provider(&mut engine, fixture_model());
        assert!(engine.board_provider().is_some());

        engine.board_refresh();
        assert!(engine.board_fetching);

        // The mock client runs synchronously on a background thread but the
        // channel send still has to be observed; poll until it lands.
        let mut tries = 0;
        while !engine.poll_board() {
            tries += 1;
            assert!(tries < 1000, "mock refresh never completed");
            std::thread::yield_now();
        }
        assert!(!engine.board_fetching);
        let model = engine.board_model.as_ref().expect("model populated");
        assert_eq!(model.columns.len(), 1);
        assert_eq!(model.columns[0].cards.len(), 2);
        assert!(engine.board_error.is_none());
    }

    #[test]
    fn board_refresh_error_surfaces_as_board_error() {
        let mut engine = Engine::new_for_test();
        let mut manifest = ExtensionManifest {
            name: "mock-board".to_string(),
            ..Default::default()
        };
        manifest.board = Some(BoardProviderConfig {
            refresh_command: vec!["mock-provider".to_string()],
            poll_interval_secs: 30,
            actions: Default::default(),
        });
        engine
            .extension_state
            .installed
            .push(crate::core::session::InstalledExtension {
                name: manifest.name.clone(),
                version: String::new(),
            });
        engine.ext_registry = Some(vec![manifest]);
        engine.set_board_client_for_test(MockToolClient(Err(ToolError::BinaryNotFound(
            "mock-provider".to_string(),
        ))));

        engine.board_refresh();
        let mut tries = 0;
        while !engine.poll_board() {
            tries += 1;
            assert!(tries < 1000, "mock refresh never completed");
            std::thread::yield_now();
        }
        assert!(engine.board_model.is_none());
        assert!(engine
            .board_error
            .as_deref()
            .unwrap()
            .contains("not found on PATH"));
    }

    #[test]
    fn apply_select_card_updates_model_selection() {
        let mut engine = Engine::new_for_test();
        engine.board_model = Some(fixture_model());
        engine.apply_board_action(BoardAction::SelectCard(WidgetId::new("card:2")));
        assert_eq!(
            engine.board_model.unwrap().selected_card_id,
            Some(WidgetId::new("card:2"))
        );
    }

    #[test]
    fn apply_open_issue_sets_status_message() {
        let mut engine = Engine::new_for_test();
        engine.board_model = Some(fixture_model());
        engine.apply_board_action(BoardAction::OpenIssue(WidgetId::new("card:2")));
        assert!(engine.message.contains("Another"));
    }

    /// #524: when a document provider is configured, `OpenIssue` opens the
    /// card as an editable markdown buffer instead of just echoing its
    /// title — "from a board card" in the issue's scope.
    #[test]
    fn apply_open_issue_opens_document_buffer_when_provider_configured() {
        use crate::core::extensions::DocumentProviderConfig;
        use crate::core::tool_client::MockToolClient;

        let mut engine = Engine::new_for_test();
        engine.board_model = Some(fixture_model());

        let mut manifest = ExtensionManifest {
            name: "mock-doc-provider".to_string(),
            ..Default::default()
        };
        manifest.document = Some(DocumentProviderConfig {
            read_command: vec!["mock".into(), "show".into(), "{id}".into()],
            write_command: vec!["mock".into(), "write".into(), "{id}".into()],
            write_follow_up: vec![],
        });
        engine
            .extension_state
            .installed
            .push(crate::core::session::InstalledExtension {
                name: manifest.name.clone(),
                version: String::new(),
            });
        engine.ext_registry = Some(vec![manifest]);
        engine.set_document_client_for_test(MockToolClient(Ok(serde_json::json!({
            "title": "Another, in full",
            "body": "Full body from the provider.\n",
        }))));

        engine.apply_board_action(BoardAction::OpenIssue(WidgetId::new("card:2")));

        assert!(
            engine.active_buffer_state().tool_document.is_some(),
            "OpenIssue should open a document buffer, not just set a message"
        );
        assert!(engine.buffer().to_string().contains("Another, in full"));
        assert!(engine
            .buffer()
            .to_string()
            .contains("Full body from the provider."));
    }

    #[test]
    fn dispatch_key_j_moves_selection_down() {
        let mut engine = Engine::new_for_test();
        engine.board_model = Some(fixture_model());
        let still_focused = engine.dispatch_board_key_unified("j");
        assert!(still_focused);
        assert_eq!(
            engine.board_model.unwrap().selected_card_id,
            Some(WidgetId::new("card:2"))
        );
    }

    #[test]
    fn dispatch_key_escape_unfocuses() {
        let mut engine = Engine::new_for_test();
        engine.board_has_focus = true;
        let still_focused = engine.dispatch_board_key_unified("Escape");
        assert!(!still_focused);
        assert!(!engine.board_has_focus);
    }

    #[test]
    fn focusing_board_panel_triggers_initial_refresh() {
        let mut engine = Engine::new_for_test();
        install_mock_provider(&mut engine, fixture_model());
        engine.focus_sidebar_panel(sidebar::PANEL_BOARD);
        assert!(engine.board_fetching, "first focus should kick off a fetch");
    }

    // ── OpenReview (#525) ────────────────────────────────────────────────

    /// Builds on [`install_mock_provider`] (manifest/`extension_state`/
    /// `ext_registry` wiring) rather than duplicating it (review nit,
    /// #525), layering on the one thing `OpenReview` needs that a plain
    /// board provider doesn't: an `actions["OpenReview"]` entry, plus
    /// swapping the mock client's response from "board refresh" JSON to
    /// "review-action" JSON for the `OpenReview` tests that follow.
    fn install_mock_provider_with_review_action(
        engine: &mut Engine,
        model: BoardModel,
        review_client_response: Result<serde_json::Value, crate::core::tool_client::ToolError>,
    ) {
        install_mock_provider(engine, model.clone());
        let registry = engine
            .ext_registry
            .as_mut()
            .expect("install_mock_provider populates the registry");
        let mut actions = std::collections::HashMap::new();
        actions.insert(
            "OpenReview".to_string(),
            vec!["mock-review".to_string(), "{id}".to_string()],
        );
        registry[0]
            .board
            .as_mut()
            .expect("install_mock_provider populates a [board] provider")
            .actions = actions;
        engine.board_model = Some(model);
        engine.set_board_client_for_test(MockToolClient(review_client_response));
    }

    /// Real temp repo with a `base` commit and a `feature` branch adding
    /// one file — enough for `open_branch_review` to actually build a
    /// non-empty change list. Returns `(repo dir, base branch name)`.
    fn init_review_repo(tag: &str) -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!(
            "board-ops-review-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
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
        std::fs::write(dir.join("base.txt"), "base\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "base"]);
        let base = crate::core::git::current_branch(&dir).unwrap();
        git(&["checkout", "-b", "feature"]);
        std::fs::write(dir.join("new.txt"), "from the reviewed branch\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "feature work"]);
        git(&["checkout", &base]);
        (dir, base)
    }

    #[test]
    fn apply_open_review_resolves_target_via_provider_and_opens_the_surface() {
        let (dir, base) = init_review_repo("happy-path");
        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        install_mock_provider_with_review_action(
            &mut engine,
            fixture_model(),
            Ok(serde_json::json!({"branch": "feature", "base": base})),
        );

        engine.apply_board_action(BoardAction::OpenReview(WidgetId::new("card:1")));

        let review = engine
            .change_review
            .as_ref()
            .expect("OpenReview should open the change-review surface");
        assert_eq!(review.entries.len(), 1);
        assert_eq!(review.entries[0].change.path, "new.txt");
        assert_eq!(
            review.entries[0].change.new_text,
            "from the reviewed branch\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_open_review_with_no_provider_sets_a_message_not_a_panic() {
        let mut engine = Engine::new_for_test();
        engine.board_model = Some(fixture_model());
        engine.apply_board_action(BoardAction::OpenReview(WidgetId::new("card:1")));
        assert!(engine.change_review.is_none());
        assert!(engine.message.contains("no provider configured"));
    }

    #[test]
    fn apply_open_review_with_no_review_command_configured_sets_a_message() {
        let mut engine = Engine::new_for_test();
        engine.board_model = Some(fixture_model());
        install_mock_provider(&mut engine, fixture_model()); // no `actions` entry at all
        engine.apply_board_action(BoardAction::OpenReview(WidgetId::new("card:1")));
        assert!(engine.change_review.is_none());
        assert!(engine.message.contains("no review command configured"));
    }

    #[test]
    fn apply_open_review_surfaces_a_failing_provider_as_a_message() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_review_action(
            &mut engine,
            fixture_model(),
            Err(crate::core::tool_client::ToolError::BinaryNotFound(
                "mock-review".to_string(),
            )),
        );
        engine.apply_board_action(BoardAction::OpenReview(WidgetId::new("card:1")));
        assert!(engine.change_review.is_none());
        assert!(engine.message.contains("not found on PATH"));
    }

    #[test]
    fn dispatch_key_shift_r_opens_review_for_the_selected_card() {
        let (dir, base) = init_review_repo("shift-r-key");
        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        install_mock_provider_with_review_action(
            &mut engine,
            fixture_model(),
            Ok(serde_json::json!({"branch": "feature", "base": base})),
        );

        let still_focused = engine.dispatch_board_key_unified("R");

        assert!(still_focused);
        assert!(
            engine.change_review.is_some(),
            "R should open the review for the selected card"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dispatch_key_shift_r_with_no_selection_is_a_noop() {
        let mut engine = Engine::new_for_test();
        engine.board_model = Some(BoardModel {
            id: WidgetId::new("board"),
            columns: vec![],
            selected_card_id: None,
            col_scroll_offset: 0,
        });
        let still_focused = engine.dispatch_board_key_unified("R");
        assert!(still_focused);
        assert!(engine.change_review.is_none());
    }
}
