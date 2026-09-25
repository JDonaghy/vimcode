//! Board panel engine plumbing (#521 Phase 0 read-only + #523
//! provider-dispatch actions).
//!
//! This module is the generic host's `core` half: it knows how to find a
//! configured provider (an installed extension whose manifest declares
//! `[board]`, #522), run it through the [`crate::core::tool_client`] seam,
//! and translate `quadraui::BoardAction`s into mutations on the cached
//! `quadraui::BoardModel`. It names **no specific external provider** — see
//! `tool_client`'s module doc for the placement rule this satisfies, which a
//! dedicated repo-root test enforces mechanically.
//!
//! ## #523: provider-declared named actions
//!
//! Beyond the read-only navigation `quadraui::BoardAction` already covers
//! (`SelectCard`, `MoveSelection`, `JumpToTop`/`JumpToBottom`, `OpenIssue`),
//! a provider can declare arbitrary named actions
//! (`extensions::BoardActionDef` — e.g. "dispatch this card's work",
//! "record a Test verdict") scoped to specific stages (columns), with an
//! optional keybinding and a confirmation requirement. Three entry points
//! run them:
//!
//! - [`Engine::open_board_context_menu`] — right-click a card, lists every
//!   action valid for its current stage; confirming an item runs
//!   [`Engine::run_board_action_by_name`] (`windows.rs`'s
//!   `context_menu_confirm`, `ContextMenuTarget::Board` arm).
//! - [`Engine::dispatch_board_key_unified`] — a single key matching a
//!   provider-declared stage keybinding for the selected card (e.g. a
//!   `P`/`S`/`F` style single-key verdict) runs it directly.
//! - `Engine::open_issue_card` — `OpenIssue` falls back to a provider
//!   action of the same name when no more specific handling applies (a
//!   `[document]` provider, #524). `OpenReview` is the one exception: its
//!   provider action resolves a review *target* rather than being
//!   fire-and-forget, so `Engine::open_review_card` reads the declared argv
//!   itself and feeds the result to `Engine::open_branch_review` (#525)
//!   instead of going through the generic dispatch below.
//!
//! [`Engine::run_board_action_by_name`] resolves the action's argv
//! (`{id}` substituted) and either dispatches it immediately or — when the
//! provider marked it `confirm` — opens a Yes/No dialog first
//! (`panels.rs`'s `"confirm_board_action"` `process_dialog_result` arm).
//! Dispatch itself is a background subprocess via the same `ToolClient`
//! seam `board_refresh` uses ([`Engine::dispatch_board_action_command`] /
//! [`Engine::poll_board_action`]); its exit status/stdout land in
//! `Engine::message`, and a successful run triggers a fresh
//! [`Engine::board_refresh`] so the panel reflects the provider's new state
//! on the next poll.

use super::*;
use crate::core::extensions::BoardProviderConfig;
use crate::core::tool_client::{self, ToolError};

/// A dispatched board action's result, as sent over `Engine::board_action_rx`
/// — the action's display label paired with its raw stdout on success (or
/// the typed error on failure). A named alias purely to keep
/// `Engine::board_action_rx`'s field type readable (clippy's
/// `type_complexity` lint).
pub type BoardActionResult = (String, Result<Vec<u8>, ToolError>);

/// Block until `ready()` observes the result of a backgrounded provider
/// command, or fail the test after a generous wall-clock deadline.
///
/// Every board command (`board_refresh`, action dispatch, freshness tick)
/// runs on a real `std::thread`, so a test that wants to see its result has
/// to wait for the OS to schedule that thread. A bounded spin on
/// `yield_now()` — what these tests used to do — is **not** a wait: with
/// nothing else runnable on this core, 1000 yields retire in microseconds
/// and the loop gives up long before the spawned thread has run at all.
/// That made the suite pass on an idle machine and fail under the
/// coordinator's parallel load, which is exactly how these tests were
/// reported broken. Sleep between attempts and bound by *time*, not by
/// iteration count, so the wait scales with how busy the box is.
#[cfg(test)]
pub(crate) fn wait_for_provider_command(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if ready() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{what} never completed within 30s"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

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

    /// Opt-in freshness (#523): periodically run the provider's declared
    /// `tick_command`, independent of whether the Board panel is currently
    /// visible — unlike [`Self::tick_board`], which only refreshes reads
    /// while the panel is active, the whole point of a tick is nudging a
    /// daemon-less provider's pipeline forward even when nobody is looking
    /// at the board. Call once per `poll_idle` tick.
    ///
    /// A no-op unless `Settings::board_tick_enabled` is set (**default
    /// off** — a passive viewer must not silently dispatch metered work)
    /// and the provider declared a non-empty `tick_command`. Fire-and-forget
    /// — the command's stdout/exit status are discarded (`ToolClient::run`),
    /// since a tick is a nudge, not a request the user is waiting on;
    /// `board_refresh`/the board panel's own poll picks up whatever changed
    /// as a result on its own cadence.
    pub(crate) fn tick_board_provider_freshness(&mut self) {
        if !self.settings.board_tick_enabled {
            return;
        }
        let Some(provider) = self.board_provider() else {
            return;
        };
        if provider.tick_command.is_empty() {
            return;
        }
        let interval = std::time::Duration::from_secs(provider.tick_interval_secs.max(1));
        let due = self.board_last_tick.is_none_or(|t| t.elapsed() >= interval);
        if !due {
            return;
        }
        self.board_last_tick = Some(std::time::Instant::now());
        let client = std::sync::Arc::clone(&self.board_client);
        let argv = provider.tick_command;
        std::thread::spawn(move || {
            let _ = client.run(&argv);
        });
    }

    /// Apply a semantic [`quadraui::BoardAction`] to the cached model.
    /// The read-only navigation actions (`SelectCard`, `MoveSelection`,
    /// `JumpToTop`/`JumpToBottom`) mutate the model directly; `OpenIssue`/
    /// `OpenReview` need `&mut self` beyond the model borrow (opening a
    /// document buffer, running a provider action) so they're resolved to
    /// an owned follow-up and handled after the match, once that borrow
    /// has ended. `ContextMenu` is never constructed by this host — a
    /// right-click opens the menu directly via
    /// [`Self::open_board_context_menu`] (pixel→cell conversion is a
    /// backend concern, done at the click site, not here) rather than
    /// round-tripping through this action enum.
    pub fn apply_board_action(&mut self, action: quadraui::BoardAction) {
        use quadraui::BoardAction;

        enum Followup {
            OpenIssue(quadraui::WidgetId),
            OpenReview(quadraui::WidgetId),
        }

        let followup = {
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
                BoardAction::OpenIssue(id) => Some(Followup::OpenIssue(id)),
                BoardAction::OpenReview(id) => Some(Followup::OpenReview(id)),
                BoardAction::ContextMenu(..) => None,
            }
        };
        match followup {
            Some(Followup::OpenIssue(id)) => self.open_issue_card(id),
            Some(Followup::OpenReview(id)) => self.open_review_card(id),
            None => {}
        }
    }

    /// `OpenIssue`'s handling, in priority order: a `[document]` provider
    /// (#524) opens the card as an editable markdown buffer; otherwise a
    /// provider-declared `"OpenIssue"` named action (#523) dispatches it;
    /// otherwise fall back to Phase 0's acknowledgement — echo the cached
    /// card's title to the status line — so a board with neither still
    /// gives feedback on open.
    fn open_issue_card(&mut self, id: quadraui::WidgetId) {
        if self.document_provider().is_some() {
            match self.open_tool_document(id.as_str()) {
                Ok(()) => {}
                Err(e) => self.message = format!("Board: {e}"),
            }
            return;
        }
        if self.run_board_action_by_name("OpenIssue", id.clone()) {
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
    /// `"OpenReview"` action ([`crate::core::extensions::BoardProviderConfig::
    /// action_by_name`]) to resolve this card to a
    /// [`crate::core::tool_client::BranchReviewTarget`], then hand that
    /// off to `Engine::open_branch_review`, which turns it into a local
    /// git diff and opens the shared change-review surface (#955). This
    /// function is the only place that knows the review comes from a board
    /// card — `open_branch_review` itself has no idea.
    ///
    /// Note this is the one named action that is **not** dispatched through
    /// [`Self::run_board_action_by_name`] (#523's generic path): its
    /// command's stdout is a review *target* to resolve into a diff, not a
    /// fire-and-forget side effect whose stdout belongs on the status line.
    ///
    /// **Blocking**, the same one-shot tradeoff `open_issue_card`'s
    /// document-provider path already made: opening a review is a
    /// deliberate user action with no cached state to show while waiting.
    fn open_review_card(&mut self, id: quadraui::WidgetId) {
        let Some(provider) = self.board_provider() else {
            self.message = "Board: no provider configured".to_string();
            return;
        };
        let argv = provider
            .action_by_name("OpenReview")
            .map(|a| a.resolve_argv(id.as_str()))
            .filter(|argv| !argv.is_empty());
        let Some(argv) = argv else {
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
            return;
        }
        // `open_branch_review` -> `open_change_review` already reset
        // `review_card_id` to `None`; set it back now that the surface it
        // opened really is for this card, so a later verdict
        // (`Engine::start_review_verdict`, #526) knows which one to report
        // against.
        self.review_card_id = Some(id.as_str().to_string());
    }

    /// The column (stage) id containing `card_id`, if it's present on the
    /// current board model. `None` for a stale id (card moved/removed
    /// since a menu/keybinding referencing it was set up) as well as "no
    /// model at all" — callers treat both the same (no-op).
    pub fn board_card_stage_id(&self, card_id: &quadraui::WidgetId) -> Option<String> {
        self.board_model
            .as_ref()?
            .columns
            .iter()
            .find(|c| c.cards.iter().any(|card| &card.id == card_id))
            .map(|c| c.id.as_str().to_string())
    }

    /// Open a context menu listing the provider-declared actions valid for
    /// `card_id`'s current stage (#523: "listing the actions the provider
    /// declares as valid for that card's stage"). `x`/`y` are cell
    /// coordinates — the same convention `Engine::open_editor_context_menu`
    /// uses; callers convert a pixel click themselves (GTK) or pass the
    /// cell position straight through (TUI).
    ///
    /// A no-op — no menu opened — when there's no provider configured, the
    /// card isn't on the current model, or the provider declared no
    /// actions valid for that card's stage; the panel stays perfectly
    /// usable read-only in that case.
    pub fn open_board_context_menu(&mut self, card_id: quadraui::WidgetId, x: u16, y: u16) {
        let Some(provider) = self.board_provider() else {
            return;
        };
        let Some(stage_id) = self.board_card_stage_id(&card_id) else {
            return;
        };
        let actions = provider.actions_for_stage(&stage_id);
        if actions.is_empty() {
            return;
        }
        let items = actions
            .iter()
            .map(|a| ContextMenuItem {
                label: a.display_label().to_string(),
                action: a.name.clone(),
                shortcut: a.key.clone().unwrap_or_default(),
                separator_after: false,
                enabled: true,
            })
            .collect();
        self.context_menu = Some(ContextMenuState {
            target: ContextMenuTarget::Board { card_id },
            items,
            selected: 0,
            screen_x: x,
            screen_y: y,
            trigger_height: 0.0,
        });
    }

    /// Run a provider-declared named action (#523) against `card_id` — from
    /// the context menu, a stage keybinding, or an `OpenIssue`/`OpenReview`
    /// fallback. Returns whether an action was actually found and started
    /// (dispatched immediately, or a confirmation dialog opened) — `false`
    /// means "no provider", "no action by that name", or "declared with an
    /// empty command" (not runnable), so callers can fall back to their own
    /// default behaviour.
    ///
    /// An action marked `confirm` opens a Yes/No dialog first (#523's
    /// "irreversible or metered actions" requirement) instead of
    /// dispatching right away — the actual dispatch happens from
    /// `process_dialog_result`'s `"confirm_board_action"` arm on "yes".
    pub fn run_board_action_by_name(
        &mut self,
        action_name: &str,
        card_id: quadraui::WidgetId,
    ) -> bool {
        let Some(provider) = self.board_provider() else {
            return false;
        };
        let Some(action) = provider.action_by_name(action_name) else {
            return false;
        };
        let argv = action.resolve_argv(card_id.as_str());
        if argv.is_empty() {
            return false;
        }
        let label = action.display_label().to_string();
        if action.confirm {
            self.pending_board_action = Some(PendingBoardAction {
                argv,
                label: label.clone(),
            });
            self.show_dialog(
                "confirm_board_action",
                "Confirm Action",
                vec![format!("Run '{label}'?")],
                vec![
                    DialogButton {
                        label: "Yes".into(),
                        hotkey: 'y',
                        action: "yes".into(),
                    },
                    DialogButton {
                        label: "Cancel".into(),
                        hotkey: '\0',
                        action: "cancel".into(),
                    },
                ],
            );
        } else {
            self.dispatch_board_action_command(argv, label);
        }
        true
    }

    /// Run `argv` on a background thread via the configured
    /// [`crate::core::tool_client::ToolClient`] — the same "spawn +
    /// mpsc, poll from `poll_idle`" pattern [`Self::board_refresh`] uses.
    /// Refuses to start a second action while one is already running
    /// (#523: metered actions shouldn't stack); the result lands via
    /// [`Self::poll_board_action`].
    pub fn dispatch_board_action_command(&mut self, argv: Vec<String>, label: String) {
        if self.board_action_running {
            self.message = "A board action is already running".to_string();
            return;
        }
        let client = std::sync::Arc::clone(&self.board_client);
        let (tx, rx) = std::sync::mpsc::channel();
        let label_for_thread = label.clone();
        std::thread::spawn(move || {
            let result = client.run_with_stdin(&argv, &[]);
            let _ = tx.send((label_for_thread, result));
        });
        self.board_action_rx = Some(rx);
        self.board_action_running = true;
        self.message = format!("Running '{label}'…");
    }

    /// Non-blocking check for a completed board action. Call from
    /// `poll_idle`. Surfaces the exit outcome (trimmed stdout on success,
    /// the typed error's message on failure) in `Engine::message` (#523's
    /// "exit status and stdout surfaced to the user"), then triggers a
    /// fresh [`Self::board_refresh`] so the panel reflects the provider's
    /// new state on the next poll (#523's "board refreshed on next poll").
    pub fn poll_board_action(&mut self) -> bool {
        let result: Option<(String, Result<Vec<u8>, ToolError>)> = self
            .board_action_rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        let Some((label, result)) = result else {
            return false;
        };
        self.board_action_running = false;
        self.board_action_rx = None;
        match result {
            Ok(stdout) => {
                let text = String::from_utf8_lossy(&stdout);
                let text = text.trim();
                self.message = if text.is_empty() {
                    format!("'{label}' completed")
                } else {
                    format!("'{label}': {text}")
                };
            }
            Err(err) => {
                self.message = format!("'{label}' failed: {}", err.user_message());
            }
        }
        self.board_refresh();
        true
    }

    /// The provider-declared action bound to `key` for the currently
    /// selected card's stage, if any — `Engine::dispatch_board_key_
    /// unified`'s lookup for #523's single-key verdict-style keybindings
    /// (e.g. `P`/`S`/`F`). `None` when there's no provider, no selected
    /// card, or no action bound to that key for the card's current stage.
    fn board_key_action(&self, key: &str) -> Option<(String, quadraui::WidgetId)> {
        let provider = self.board_provider()?;
        let card_id = self.board_model.as_ref()?.selected_card_id.clone()?;
        let stage_id = self.board_card_stage_id(&card_id)?;
        provider
            .action_for_key(key, &stage_id)
            .map(|a| (a.name.clone(), card_id))
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
    /// Two host-level keymaps sit in front of that generic one, in this
    /// precedence order:
    ///
    /// 1. `R` (#525) opens the review for the selected card —
    ///    `handle_key`'s own doc explicitly calls "review" out as a
    ///    workflow-specific verb hosts should handle themselves, so it's
    ///    dispatched here rather than added to quadraui's generic keymap.
    ///    It deliberately wins over a provider that also declares `R` as a
    ///    stage keybinding: a provider's `"OpenReview"` command returns a
    ///    review *target* to resolve (see [`Self::open_review_card`]), not
    ///    a fire-and-forget command, so letting the generic dispatch path
    ///    claim `R` would silently downgrade opening a review to printing
    ///    that JSON to the status line.
    /// 2. A key matching a provider-declared stage keybinding for the
    ///    selected card (#523) is checked *before* falling to
    ///    `BoardModel::handle_key`'s generic navigation, so a provider is
    ///    free to bind e.g. `P`/`S`/`F` without them ever reaching the
    ///    primitive's own keymap.
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
        // Both backends' `render::engine_key_from_ui` translates
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
        if let Some((action_name, card_id)) = self.board_key_action(key) {
            self.run_board_action_by_name(&action_name, card_id);
            return true;
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
        wait_for_provider_command("mock refresh", || engine.poll_board());
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
        engine.set_board_client_for_test(MockToolClient(Err(ToolError::BinaryNotFound(
            "mock-provider".to_string(),
        ))));

        engine.board_refresh();
        wait_for_provider_command("mock refresh", || engine.poll_board());
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
    /// board provider doesn't: an `"OpenReview"` [`BoardActionDef`], plus
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
        registry[0]
            .board
            .as_mut()
            .expect("install_mock_provider populates a [board] provider")
            .actions = vec![BoardActionDef {
            name: "OpenReview".to_string(),
            command: vec!["mock-review".to_string(), "{id}".to_string()],
            ..Default::default()
        }];
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

    // ─── #523: provider-declared named actions ─────────────────────────────

    use crate::core::extensions::BoardActionDef;
    use crate::core::tool_client::RecordingToolClient;

    /// Install a mock `[board]` provider (like `install_mock_provider`) that
    /// additionally declares `actions`. No coordinator (or any other
    /// specific provider) vocabulary anywhere in this test, per #522/#523's
    /// "generic host" scope. Also sets `engine.board_model` directly
    /// (synchronously) — #523's tests exercise action dispatch against an
    /// already-loaded model, not the async refresh flow
    /// `board_refresh_with_mock_provider_populates_model` covers.
    fn install_mock_provider_with_actions(
        engine: &mut Engine,
        model: BoardModel,
        actions: Vec<BoardActionDef>,
    ) {
        let mut manifest = ExtensionManifest {
            name: "mock-board".to_string(),
            ..Default::default()
        };
        manifest.board = Some(BoardProviderConfig {
            refresh_command: vec!["mock-provider".to_string()],
            poll_interval_secs: 30,
            actions,
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
        let json = serde_json::to_value(&model).unwrap();
        engine.set_board_client_for_test(MockToolClient(Ok(json)));
        engine.board_model = Some(model);
    }

    /// Poll until a dispatched board action's result has landed, mirroring
    /// `board_refresh_with_mock_provider_populates_model`'s own poll loop
    /// (the background thread's send still has to be observed).
    fn wait_for_board_action(engine: &mut Engine) {
        wait_for_provider_command("board action", || engine.poll_board_action());
    }

    /// The argv of every recorded call that is **not** the mock provider's
    /// `refresh_command`.
    ///
    /// [`Engine::poll_board_action`] deliberately kicks off a follow-up
    /// [`Engine::board_refresh`] once an action's result lands (#523's
    /// "board refreshed on next poll"), and that refresh runs on its own
    /// background thread through this same client. Whether it has reached
    /// the recorder by the time the test reads the log is a race against
    /// the OS scheduler — `recorder.calls().len()` is legitimately 1 *or*
    /// 2 — so an assertion about *the action* must not count it.
    /// `poll_board_action_triggers_a_follow_up_provider_refresh` covers the
    /// refresh itself, deterministically, by waiting for it.
    fn action_argvs(recorder: &RecordingToolClient) -> Vec<Vec<String>> {
        recorder
            .calls()
            .into_iter()
            .map(|c| c.argv)
            .filter(|argv| argv.first().map(String::as_str) != Some("mock-provider"))
            .collect()
    }

    fn dispatch_action(name: &str, command: Vec<&str>) -> BoardActionDef {
        BoardActionDef {
            name: name.to_string(),
            label: String::new(),
            command: command.into_iter().map(str::to_string).collect(),
            stages: vec![],
            key: None,
            confirm: false,
        }
    }

    #[test]
    fn open_board_context_menu_lists_only_actions_valid_for_the_cards_stage() {
        let mut engine = Engine::new_for_test();
        let mut model = fixture_model();
        model.columns.push(BoardColumn {
            id: WidgetId::new("col:test"),
            title: "Test".into(),
            cards: vec![card("card:3", "Third")],
            scroll_offset: 0,
        });
        install_mock_provider_with_actions(
            &mut engine,
            model,
            vec![
                dispatch_action("assign", vec!["mock", "assign", "{id}"]),
                BoardActionDef {
                    name: "test-pass".into(),
                    label: "Mark Passed".into(),
                    command: vec!["mock".into(), "test".into(), "{id}".into()],
                    stages: vec!["col:test".into()],
                    key: Some("P".into()),
                    confirm: false,
                },
            ],
        );

        // A card in "col:backlog": only the stage-agnostic "assign" applies.
        engine.open_board_context_menu(WidgetId::new("card:1"), 3, 4);
        let menu = engine.context_menu.take().expect("menu opened");
        assert_eq!(menu.items.len(), 1);
        assert_eq!(menu.items[0].action, "assign");
        assert_eq!(menu.screen_x, 3);
        assert_eq!(menu.screen_y, 4);

        // A card in "col:test": both apply.
        engine.open_board_context_menu(WidgetId::new("card:3"), 0, 0);
        let menu = engine.context_menu.take().expect("menu opened");
        let names: Vec<&str> = menu.items.iter().map(|i| i.action.as_str()).collect();
        assert!(names.contains(&"assign"));
        assert!(names.contains(&"test-pass"));
        assert_eq!(
            menu.items
                .iter()
                .find(|i| i.action == "test-pass")
                .unwrap()
                .shortcut,
            "P"
        );
    }

    #[test]
    fn open_board_context_menu_is_a_no_op_with_no_actions_for_the_stage() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_actions(
            &mut engine,
            fixture_model(),
            vec![BoardActionDef {
                name: "test-pass".into(),
                stages: vec!["col:test".into()],
                ..dispatch_action("test-pass", vec!["mock"])
            }],
        );
        engine.open_board_context_menu(WidgetId::new("card:1"), 0, 0);
        assert!(
            engine.context_menu.is_none(),
            "'card:1' is in 'col:backlog', which the only declared action doesn't cover"
        );
    }

    #[test]
    fn run_board_action_by_name_dispatches_and_surfaces_result() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_actions(
            &mut engine,
            fixture_model(),
            vec![dispatch_action("assign", vec!["mock", "assign", "{id}"])],
        );
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());

        let started = engine.run_board_action_by_name("assign", WidgetId::new("card:2"));
        assert!(started);
        wait_for_board_action(&mut engine);

        let calls = action_argvs(&recorder);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            vec![
                "mock".to_string(),
                "assign".to_string(),
                "card:2".to_string()
            ],
            "'{{id}}' must be substituted with the acted-on card's id"
        );
        assert!(
            engine.message.contains("assign"),
            "exit status/stdout must be surfaced to the user; message: {}",
            engine.message
        );
    }

    /// The other half of [`Engine::poll_board_action`] (#523's "board
    /// refreshed on next poll"): once the action's result lands, the
    /// provider's `refresh_command` must be dispatched too, so the panel
    /// reflects whatever the action changed. Waited on explicitly rather
    /// than asserted as a call count — the refresh runs on its own thread,
    /// which is exactly why `action_argvs` filters it out elsewhere.
    #[test]
    fn poll_board_action_triggers_a_follow_up_provider_refresh() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_actions(
            &mut engine,
            fixture_model(),
            vec![dispatch_action("assign", vec!["mock", "assign", "{id}"])],
        );
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());

        assert!(engine.run_board_action_by_name("assign", WidgetId::new("card:2")));
        wait_for_board_action(&mut engine);

        wait_for_provider_command("follow-up board refresh", || {
            recorder
                .calls()
                .iter()
                .any(|c| c.argv == vec!["mock-provider".to_string()])
        });
    }

    #[test]
    fn run_board_action_by_name_with_no_provider_action_is_a_no_op() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_actions(&mut engine, fixture_model(), vec![]);
        assert!(!engine.run_board_action_by_name("assign", WidgetId::new("card:1")));
        assert!(!engine.board_action_running);
    }

    #[test]
    fn run_board_action_by_name_with_confirm_opens_dialog_and_waits() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_actions(
            &mut engine,
            fixture_model(),
            vec![BoardActionDef {
                name: "merge".into(),
                label: "Merge".into(),
                command: vec!["mock".into(), "merge".into(), "{id}".into()],
                stages: vec![],
                key: None,
                confirm: true,
            }],
        );
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());

        let started = engine.run_board_action_by_name("merge", WidgetId::new("card:1"));
        assert!(started);
        assert!(
            engine.dialog.is_some(),
            "a `confirm: true` action must open a dialog, not dispatch immediately"
        );
        assert!(
            recorder.calls().is_empty(),
            "nothing should run before the dialog is confirmed"
        );

        // Confirm via the real dialog-result path.
        let dlg = engine.dialog.take().unwrap();
        engine.process_dialog_result(&dlg.tag, "yes", None);
        wait_for_board_action(&mut engine);

        let calls = action_argvs(&recorder);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            vec![
                "mock".to_string(),
                "merge".to_string(),
                "card:1".to_string()
            ]
        );
    }

    #[test]
    fn run_board_action_by_name_confirm_cancel_never_dispatches() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_actions(
            &mut engine,
            fixture_model(),
            vec![BoardActionDef {
                name: "merge".into(),
                confirm: true,
                ..dispatch_action("merge", vec!["mock", "merge", "{id}"])
            }],
        );
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());

        engine.run_board_action_by_name("merge", WidgetId::new("card:1"));
        let dlg = engine.dialog.take().unwrap();
        engine.process_dialog_result(&dlg.tag, "cancel", None);

        assert!(recorder.calls().is_empty());
        assert!(engine.pending_board_action.is_none());
        assert!(!engine.board_action_running);
    }

    #[test]
    fn dispatch_board_key_unified_runs_provider_stage_keybinding() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_actions(
            &mut engine,
            fixture_model(),
            vec![BoardActionDef {
                name: "test-pass".into(),
                label: "Mark Passed".into(),
                command: vec!["mock".into(), "pass".into(), "{id}".into()],
                stages: vec!["col:backlog".into()],
                key: Some("P".into()),
                confirm: false,
            }],
        );
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());
        // `fixture_model` selects "card:1", which is in "col:backlog".

        let still_focused = engine.dispatch_board_key_unified("P");
        assert!(still_focused);
        wait_for_board_action(&mut engine);

        let calls = action_argvs(&recorder);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            vec!["mock".to_string(), "pass".to_string(), "card:1".to_string()]
        );
    }

    #[test]
    fn dispatch_board_key_unified_falls_back_to_navigation_when_key_not_bound() {
        // A provider-declared keybinding takes priority, but a key nothing
        // binds still reaches `quadraui::BoardModel::handle_key`'s own
        // generic navigation — provider actions augment, they don't break,
        // Phase 0's read-only navigation.
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_actions(
            &mut engine,
            fixture_model(),
            vec![BoardActionDef {
                name: "test-pass".into(),
                key: Some("P".into()),
                ..dispatch_action("test-pass", vec!["mock"])
            }],
        );
        engine.dispatch_board_key_unified("j");
        assert_eq!(
            engine.board_model.as_ref().unwrap().selected_card_id,
            Some(WidgetId::new("card:2"))
        );
    }

    #[test]
    fn context_menu_confirm_on_board_target_runs_the_selected_action() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_actions(
            &mut engine,
            fixture_model(),
            vec![dispatch_action("assign", vec!["mock", "assign", "{id}"])],
        );
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());

        engine.open_board_context_menu(WidgetId::new("card:2"), 1, 1);
        assert!(engine.context_menu.is_some());

        let confirmed = engine.context_menu_confirm();
        assert_eq!(confirmed.as_deref(), Some("assign"));
        assert!(engine.context_menu.is_none(), "confirming closes the menu");
        wait_for_board_action(&mut engine);

        assert_eq!(
            recorder.calls()[0].argv,
            vec![
                "mock".to_string(),
                "assign".to_string(),
                "card:2".to_string()
            ]
        );
    }

    /// `"OpenReview"` is the one provider-declared action name that must
    /// **not** flow through #523's generic fire-and-forget dispatch: #525
    /// resolves its stdout into a
    /// [`crate::core::tool_client::BranchReviewTarget`] and opens the
    /// change-review surface instead (see `Engine::open_review_card`).
    /// Pinning the precedence here because the two features were developed
    /// in parallel and both claim the same action name — a regression would
    /// silently turn "open the review" back into "print the review JSON to
    /// the status line".
    #[test]
    fn apply_open_review_resolves_a_target_instead_of_dispatching_the_action() {
        let (dir, base) = init_review_repo("precedence-over-dispatch");
        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        install_mock_provider_with_review_action(
            &mut engine,
            fixture_model(),
            Ok(serde_json::json!({"branch": "feature", "base": base})),
        );

        engine.apply_board_action(BoardAction::OpenReview(WidgetId::new("card:1")));

        assert!(
            engine.change_review.is_some(),
            "OpenReview must open the change-review surface (#525)"
        );
        assert!(
            !engine.board_action_running,
            "OpenReview must not be routed through the generic board-action \
             dispatcher (#523) — message was {:?}",
            engine.message
        );
        assert!(
            !engine.message.starts_with("Running '"),
            "OpenReview must not surface as a fire-and-forget dispatch; \
             message: {:?}",
            engine.message
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_open_issue_falls_back_to_provider_action_when_no_document_provider() {
        // No `[document]` provider configured, but the board provider does
        // declare an "OpenIssue" action — #523's dispatch should be tried
        // before falling all the way back to the plain title-echo message.
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_actions(
            &mut engine,
            fixture_model(),
            vec![dispatch_action("OpenIssue", vec!["mock", "open", "{id}"])],
        );
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());

        engine.apply_board_action(BoardAction::OpenIssue(WidgetId::new("card:2")));
        wait_for_board_action(&mut engine);

        assert_eq!(
            recorder.calls()[0].argv,
            vec!["mock".to_string(), "open".to_string(), "card:2".to_string()]
        );
    }

    // ─── #523: opt-in freshness (`tick_command`) ───────────────────────────

    fn install_mock_provider_with_tick(engine: &mut Engine, tick_interval_secs: u64) {
        let mut manifest = ExtensionManifest {
            name: "mock-board".to_string(),
            ..Default::default()
        };
        manifest.board = Some(BoardProviderConfig {
            refresh_command: vec!["mock-provider".to_string()],
            poll_interval_secs: 30,
            tick_command: vec!["mock".to_string(), "notify".to_string()],
            tick_interval_secs,
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

    /// Wait until the recorder has observed at least one call — there is no
    /// channel/receiver for a tick (it's genuinely fire-and-forget), so this
    /// waits on the recorder itself rather than an `Engine::poll_*` method.
    fn wait_for_recorded_call(recorder: &RecordingToolClient) {
        wait_for_provider_command("tick command", || !recorder.calls().is_empty());
    }

    #[test]
    fn tick_board_provider_freshness_is_off_by_default() {
        let mut engine = Engine::new_for_test();
        install_mock_provider_with_tick(&mut engine, 1);
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());
        assert!(!engine.settings.board_tick_enabled);

        engine.tick_board_provider_freshness();

        // Give a background thread every chance to have fired anyway before
        // asserting nothing did — flakiness here would hide a real bug
        // (dispatching metered work with no opt-in), not save time.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            recorder.calls().is_empty(),
            "a passive viewer must not silently dispatch metered work"
        );
    }

    #[test]
    fn tick_board_provider_freshness_runs_when_enabled_and_due() {
        let mut engine = Engine::new_for_test();
        engine.settings.board_tick_enabled = true;
        install_mock_provider_with_tick(&mut engine, 1);
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());

        engine.tick_board_provider_freshness();
        wait_for_recorded_call(&recorder);

        assert_eq!(
            recorder.calls()[0].argv,
            vec!["mock".to_string(), "notify".to_string()]
        );
    }

    #[test]
    fn tick_board_provider_freshness_respects_the_interval() {
        let mut engine = Engine::new_for_test();
        engine.settings.board_tick_enabled = true;
        // A long interval so the second call below is unambiguously "not
        // due yet" rather than a race against real time.
        install_mock_provider_with_tick(&mut engine, 3600);
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());

        engine.tick_board_provider_freshness();
        wait_for_recorded_call(&recorder);
        engine.tick_board_provider_freshness();

        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(
            recorder.calls().len(),
            1,
            "a second tick within the declared interval must not re-fire"
        );
    }

    #[test]
    fn tick_board_provider_freshness_with_no_tick_command_is_a_no_op() {
        let mut engine = Engine::new_for_test();
        engine.settings.board_tick_enabled = true;
        install_mock_provider(&mut engine, fixture_model());
        let recorder = RecordingToolClient::default();
        engine.set_board_client_for_test(recorder.clone());

        engine.tick_board_provider_freshness();

        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(recorder.calls().is_empty());
    }
}
