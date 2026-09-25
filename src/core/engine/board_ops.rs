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
    /// `MoveSelection`, `JumpToTop`/`JumpToBottom`, `OpenIssue`); every
    /// other variant is a provider-dispatch action out of scope until #523
    /// and is a deliberate no-op here.
    pub fn apply_board_action(&mut self, action: quadraui::BoardAction) {
        use quadraui::BoardAction;
        // `OpenIssue` needs `&mut self` (to open a document buffer, #524),
        // which can't happen while `model` still holds `self.board_model`
        // borrowed — resolve it to an owned id first and handle it after
        // the match, once that borrow has ended.
        let open_issue_id = {
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
                BoardAction::OpenIssue(id) => Some(id),
                // Provider-dispatch actions (#523) and context menu —
                // no-op in this read-only phase.
                BoardAction::ContextMenu(..) | BoardAction::OpenReview(_) => None,
            }
        };
        let Some(id) = open_issue_id else {
            return;
        };
        self.open_issue_card(id);
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

    /// Keyboard dispatch for the Board panel — the
    /// `dispatch_*_sidebar_key_unified` pattern every other panel uses.
    ///
    /// `Escape` leaves the panel (mirrors every sibling panel's exit key);
    /// `h`/`Left` and `l`/`Right` are left to `quadraui::BoardModel::
    /// handle_key`'s own generic keymap for column navigation instead,
    /// since (unlike the other panels) they are meaningful board
    /// navigation here, not an "exit to the activity bar" gesture.
    ///
    /// Returns whether the panel is still focused afterward.
    pub fn dispatch_board_key_unified(&mut self, key: &str) -> bool {
        if key == "Escape" {
            self.board_has_focus = false;
            return false;
        }
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
}
