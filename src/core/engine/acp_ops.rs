//! `Engine::poll_acp` — the one call site `poll_idle` uses to drain the ACP
//! transport (#951 ACP-0 built the transport; #952 ACP-1 adds the session
//! lifecycle drive and the `session/update` -> AI-panel chunk mapping this
//! module implements; #953 ACP-2 adds `session/request_permission` — the
//! human-in-the-loop tool-call approval chokepoint — routed onto the
//! existing dialog system, plus `session/cancel` wiring; #954 ACP-3 adds
//! `fs/read_text_file`/`fs/write_text_file`, served through vimcode's own
//! buffers rather than the raw filesystem — see [`Engine::acp_read_text_file`]
//! / [`Engine::acp_write_text_file`]). `tool_call`/`tool_call_update`/`plan`
//! updates are still intentionally left unhandled here — ignored without
//! breaking the stream — per ACP-1's scope; ACP-4/5 add real handling for
//! those on top of the same `AcpEvent` stream, not a transport change.

use super::*;
use crate::core::acp::{AcpChunkKind, AcpEvent};

impl Engine {
    /// Non-blocking drain of the live ACP agent's events, if one is
    /// running. Returns `true` if a redraw is needed.
    pub fn poll_acp(&mut self) -> bool {
        let Some(client) = self.acp_client.as_mut() else {
            return false;
        };
        let events = client.poll();
        if events.is_empty() {
            return false;
        }

        let mut redraw = false;
        for event in events {
            match event {
                AcpEvent::AgentExited {
                    stderr,
                    was_initialized,
                } => {
                    // Reuse the same generic status-line message both
                    // backends already render for LSP exits
                    // (`LspEvent::ServerExited` in `panels.rs`) — no new
                    // backend-specific surface needed.
                    if was_initialized {
                        self.message = "ACP agent exited".to_string();
                    } else {
                        let snippet: String = stderr.chars().take(200).collect();
                        self.message = format!("ACP agent failed to start: {snippet}");
                        self.ai_messages.push(AiMessage {
                            role: "assistant-thought".to_string(),
                            content: format!("\u{26a0} ACP agent failed to start: {snippet}"),
                        });
                    }
                    // The agent is gone — its stdin is a dead pipe, so this
                    // must NOT go through `acp_cancel_pending_permission`
                    // (which writes a reply). Just close whatever dialog was
                    // parked on it so it doesn't linger on screen forever
                    // (#953's "agent death with a dialog open" acceptance
                    // criterion) and drop the bookkeeping — there is no one
                    // left to answer.
                    if self.acp_pending_permission.take().is_some()
                        && self
                            .dialog
                            .as_ref()
                            .is_some_and(|d| d.tag == "acp_permission")
                    {
                        self.dialog = None;
                    }
                    self.acp_remembered_decisions.clear();
                    self.acp_client = None;
                    self.acp_session_id = None;
                    self.acp_pending_prompt = None;
                    // #1459: a resume the agent died before completing must
                    // not silently apply to whatever agent starts next, and
                    // its deferred display line (if any) goes with it — the
                    // agent is gone, so there's no transcript left for it to
                    // land after.
                    self.acp_pending_resume = None;
                    self.acp_pending_prompt_display = None;
                    self.acp_streaming_turn = None;
                    // #956 (ACP-5): session-scoped, same as the decisions
                    // map above — see `Engine::ai_clear`'s matching reset.
                    self.acp_plan.clear();
                    self.acp_available_commands.clear();
                    self.acp_command_completion_idx = 0;
                    self.acp_modes.clear();
                    self.acp_current_mode_id = None;
                    self.acp_usage = None;
                    // #957 (ACP-6): session-scoped, same as the rest above.
                    self.acp_auth_methods.clear();
                    self.acp_authenticated = false;
                    self.acp_prompt_capabilities =
                        crate::core::acp::AcpPromptCapabilities::default();
                    // #955 (ACP-4): session-scoped, same as the rest above
                    // — see `Engine::ai_clear`'s matching reset.
                    self.acp_tool_calls.clear();
                    self.change_review = None;
                    self.ai_streaming = false;
                    redraw = true;
                }
                AcpEvent::Initialized {
                    auth_methods,
                    agent_capabilities,
                    ..
                } => {
                    // #1449: capture `promptCapabilities` before anything
                    // else touches this event — both the auth-choice and
                    // straight-to-session branches below need it available
                    // for the first `session/prompt` either way, and
                    // `agent_capabilities` isn't read again after this.
                    self.acp_prompt_capabilities =
                        crate::core::acp::parse_prompt_capabilities(&agent_capabilities);
                    // #1459: cache whether this agent supports `session/
                    // load` — keyed by agent name so `:AiSessions` can
                    // answer without spawning the agent first, and
                    // persisted so the answer survives a vimcode restart.
                    let load_session_supported =
                        crate::core::acp::parse_load_session_capability(&agent_capabilities);
                    let active_agent_name = self.acp_active_agent_name();
                    self.acp_session_index
                        .set_load_session_capability(&active_agent_name, load_session_supported);
                    self.acp_session_index.save();
                    // Handshake step 2. #957 (ACP-6): if the agent offers
                    // any `authMethods` and this client hasn't resolved
                    // auth yet for this session (skipped, a `type: "agent"`
                    // method succeeded, or a `type: "terminal"` login
                    // exited `0` — see `acp_authenticated`'s doc), present
                    // the choice instead of opening a session straight
                    // away. Otherwise (no auth methods at all — every
                    // pre-#957 agent and test — or auth already resolved,
                    // e.g. this is the re-`initialize()` after a successful
                    // terminal login) proceed exactly as before.
                    self.acp_auth_methods = crate::core::acp::parse_auth_methods(&auth_methods);
                    if !self.acp_authenticated && !self.acp_auth_methods.is_empty() {
                        self.acp_show_auth_choice();
                    } else {
                        self.acp_begin_session();
                    }
                    redraw = true;
                }
                AcpEvent::Authenticated { .. } => {
                    // #957 (ACP-6): a `type: "agent"` auth method
                    // succeeded — proceed exactly like an agent that never
                    // required auth in the first place. A failure instead
                    // comes through `AcpEvent::RequestFailed` below, whose
                    // existing generic handling already clears the busy
                    // state and surfaces a message — no special-casing
                    // needed there.
                    self.acp_authenticated = true;
                    self.message = "Authenticated.".to_string();
                    self.acp_begin_session();
                    redraw = true;
                }
                AcpEvent::SessionCreated {
                    session_id, modes, ..
                } => {
                    self.acp_session_id = Some(session_id.clone());
                    // #1459: remember this session (id, agent, cwd, first
                    // prompt) so `:AiSessions` can offer it again in a later
                    // run — `acp_pending_prompt` is read, not taken, so the
                    // "send the queued prompt" branch below still sees it.
                    {
                        let agent_name = self.acp_active_agent_name();
                        let cwd = self.acp_workspace_cwd();
                        let first_prompt = self.acp_pending_prompt.clone().unwrap_or_default();
                        self.acp_session_index.record_session(
                            &session_id,
                            &agent_name,
                            &cwd,
                            &first_prompt,
                        );
                        self.acp_session_index.save();
                    }
                    // #956 (ACP-5): `session/new`'s optional `modes` field —
                    // an agent that doesn't support modes at all simply omits
                    // it, which `parse_session_modes` treats as "no modes",
                    // not an error.
                    if let Some(modes_json) = modes {
                        let (current, list) = crate::core::acp::parse_session_modes(&modes_json);
                        self.acp_modes = list;
                        self.acp_current_mode_id = current;
                    }
                    if let Some(text) = self.acp_pending_prompt.take() {
                        // #1449: rebuilt fresh here (rather than carrying
                        // pre-built blocks in `acp_pending_prompt` itself)
                        // so the attachment reflects whatever's the active
                        // buffer *now*, at the moment the handshake
                        // actually completes — computed before the
                        // `acp_client` borrow below, since both need
                        // `&self`/`&mut self` on the same field.
                        let content = self.acp_prompt_content_blocks(&text);
                        if let Some(client) = self.acp_client.as_mut() {
                            client.prompt(&session_id, content);
                        }
                    } else {
                        // No prompt was waiting on this handshake — nothing
                        // to stream, so the panel shouldn't sit "thinking".
                        self.ai_streaming = false;
                    }
                    redraw = true;
                }
                AcpEvent::SessionLoaded { modes, .. } => {
                    // #1459: `acp_session_id` was already set to the
                    // resumed id by `Engine::acp_begin_session` before the
                    // `session/load` request was even sent — see that
                    // function's doc for why (a resuming agent may emit
                    // the session's history as `session/update`
                    // notifications before this response line arrives, and
                    // `SessionUpdate`'s handler drops updates for a session
                    // id it doesn't recognise yet). This handler only needs
                    // to finish what `SessionCreated` does after that:
                    // apply any `modes` the response carries, then send
                    // whatever prompt was waiting on the handshake (the
                    // `acp_reopen_last_session` "resume, then send the
                    // typed message" path) or clear the busy state if none
                    // was.
                    if let Some(modes_json) = modes {
                        let (current, list) = crate::core::acp::parse_session_modes(&modes_json);
                        self.acp_modes = list;
                        self.acp_current_mode_id = current;
                    }
                    if let Some(session_id) = self.acp_session_id.clone() {
                        // #1459 review: refresh this record's `updated_at`
                        // (idempotent by id, see `record_session`'s doc) so a
                        // session just resumed and used again bubbles back
                        // to the top of a later `:AiSessions` picker instead
                        // of looking stale next to sessions that were merely
                        // created, never resumed.
                        let agent_name = self.acp_active_agent_name();
                        let cwd = self.acp_workspace_cwd();
                        self.acp_session_index
                            .record_session(&session_id, &agent_name, &cwd, "");
                        self.acp_session_index.save();
                        // #1459 review: a message deferred by the
                        // `acp_reopen_last_session` auto-resume path
                        // (`ai_send_message_via_acp`) is shown now — only
                        // after the replayed `session/update` history above
                        // has finished landing in `ai_messages` — so it
                        // appears *after* the "past" conversation it's
                        // continuing, not spliced in ahead of it.
                        if let Some(display) = self.acp_pending_prompt_display.take() {
                            self.ai_messages.push(AiMessage {
                                role: "user".to_string(),
                                content: display,
                            });
                        }
                        if let Some(text) = self.acp_pending_prompt.take() {
                            let content = self.acp_prompt_content_blocks(&text);
                            if let Some(client) = self.acp_client.as_mut() {
                                client.prompt(&session_id, content);
                            }
                        } else {
                            self.ai_streaming = false;
                            self.message = "Session resumed.".to_string();
                        }
                    }
                    redraw = true;
                }
                AcpEvent::SessionUpdate { session_id, update } => {
                    // `update` here is the whole `session/update` notification
                    // `params` object (`{"sessionId": ..., "update": {...}}` —
                    // see `AcpEvent::SessionUpdate`'s doc in `core::acp`), not
                    // the inner tagged union `session_update_chunk` parses;
                    // unwrap one level first.
                    let inner = update.get("update");
                    if self.acp_session_id.as_deref() == Some(session_id.as_str()) {
                        if let Some(inner) = inner {
                            self.acp_handle_session_update(inner);
                        }
                    }
                    redraw = true;
                }
                AcpEvent::PromptStopped { stop_reason, .. } => {
                    self.ai_streaming = false;
                    self.acp_streaming_turn = None;
                    if stop_reason != "end_turn" {
                        self.ai_messages.push(AiMessage {
                            role: "assistant-thought".to_string(),
                            content: format!("[turn stopped: {stop_reason}]"),
                        });
                    }
                    redraw = true;
                }
                AcpEvent::RequestFailed {
                    method, message, ..
                } => {
                    // Any non-fatal JSON-RPC error while a turn is in
                    // flight must clear the busy state, not just a failed
                    // `session/prompt` — an `initialize` or `session/new`
                    // that errors without exiting the process (bad `cwd`,
                    // protocol mismatch, ...) would otherwise leave
                    // `ai_streaming` stuck `true` forever: the warning below
                    // lands in the transcript, but the spinner never clears
                    // and `ai_send_message` silently no-ops on every
                    // subsequent call (see `ext_panel.rs`'s early return on
                    // `self.ai_streaming`).
                    self.ai_streaming = false;
                    self.acp_streaming_turn = None;
                    self.message = format!("ACP {method} failed: {message}");
                    self.ai_messages.push(AiMessage {
                        role: "assistant-thought".to_string(),
                        content: format!("\u{26a0} {method} failed: {message}"),
                    });
                    if method == "session/load" {
                        // #1459 review: `acp_begin_session`'s resume branch
                        // sets `acp_session_id` to the resumed id *before*
                        // the request goes out (see its doc, and
                        // `ACP_FAKE_LOAD_SESSION_ERROR`'s fixture doc for the
                        // exact "agent doesn't actually still have this
                        // session" regression this guards). If the agent
                        // then rejects the load, that id must not linger —
                        // the agent never actually created it, so every
                        // later `session/prompt` against it would fail the
                        // same way, silently and forever. Reset it and fall
                        // back to a fresh session instead, matching
                        // `acp_reopen_last_session`'s own doc ("silently
                        // falls back to a fresh session"): any message the
                        // auto-resume path deferred display of is shown now
                        // (there is no history left to land it after), and
                        // `acp_begin_session` — with `acp_pending_resume`
                        // already consumed by the failed attempt — takes its
                        // no-pending-resume branch and sends a plain
                        // `session/new`, which will pick `acp_pending_prompt`
                        // back up via `SessionCreated` exactly like a cold
                        // start.
                        self.acp_session_id = None;
                        if let Some(display) = self.acp_pending_prompt_display.take() {
                            self.ai_messages.push(AiMessage {
                                role: "user".to_string(),
                                content: display,
                            });
                        }
                        if self.acp_pending_prompt.is_some() {
                            self.ai_streaming = true;
                        }
                        self.acp_begin_session();
                    } else {
                        // `acp_pending_prompt` is dropped for every other
                        // failure so a queued prompt from a failed handshake
                        // isn't replayed against a later, unrelated session
                        // — `session/load` is the one exception, handled
                        // above, where the fallback session is what it's
                        // meant to reach.
                        self.acp_pending_prompt = None;
                        self.acp_pending_prompt_display = None;
                    }
                    redraw = true;
                }
                AcpEvent::ClientRequest {
                    request_id,
                    method,
                    params,
                } => {
                    match method.as_str() {
                        "session/request_permission" => {
                            self.acp_handle_permission_request(request_id, params);
                        }
                        "fs/read_text_file" => {
                            self.acp_handle_read_text_file(request_id, params);
                        }
                        "fs/write_text_file" => {
                            self.acp_handle_write_text_file(request_id, params);
                        }
                        // Everything else is still left parked, not
                        // answered — not answering doesn't break the
                        // transport (`AcpClient::poll` keeps draining), it
                        // just means a turn that needs one will not reach
                        // `PromptStopped` yet.
                        _ => {}
                    }
                    redraw = true;
                }
            }
        }
        redraw
    }

    /// Absolute directory handed to `session/new`'s `cwd` — the workspace
    /// root if one is open, else the process's own cwd. `AcpClient::
    /// new_session` canonicalizes it, but resolving *which* directory is
    /// engine/workspace policy, not transport plumbing.
    ///
    /// `pub(crate)` so `ext_panel.rs`'s `ai_send_message_via_acp` (which
    /// needs the same cwd to spawn the agent in the first place, before any
    /// `Initialized` event exists to drive this module's handler) can reuse
    /// it instead of re-deriving the same fallback inline.
    pub(crate) fn acp_workspace_cwd(&self) -> std::path::PathBuf {
        self.workspace_root
            .clone()
            .unwrap_or_else(|| self.cwd.clone())
    }

    // ── multi-agent registry (#958, ACP-7) ───────────────────────────────────

    /// Name of the entry in `settings.acp_agents` that is currently active.
    /// `settings.acp_active_agent` if it still names a real entry, else the
    /// registry's first entry, else empty (meaning "no registry configured
    /// — fall back to the legacy `acp_agent_command` single string").
    pub(crate) fn acp_active_agent_name(&self) -> String {
        let want = self.settings.acp_active_agent.trim();
        if !want.is_empty()
            && self
                .settings
                .acp_agents
                .iter()
                .any(|a| a.name.eq_ignore_ascii_case(want))
        {
            return want.to_string();
        }
        self.settings
            .acp_agents
            .first()
            .map(|a| a.name.clone())
            .unwrap_or_default()
    }

    /// The active `AcpAgentProfile`, if the registry is non-empty.
    fn acp_active_agent_profile(&self) -> Option<&crate::core::acp::AcpAgentProfile> {
        let name = self.acp_active_agent_name();
        if name.is_empty() {
            return None;
        }
        self.settings
            .acp_agents
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(&name))
    }

    /// Resolve what to hand `AcpClient::spawn_with_env` for the *next*
    /// agent spawn: `argv`, `cwd`, extra `env`, and a human-readable label
    /// for error messages. Reads the registry (`settings.acp_agents` +
    /// `acp_active_agent`) when non-empty; otherwise falls back to the
    /// pre-#958 single-string `acp_agent_command`, unchanged. This is the
    /// **one** place that knows about the registry at all — a second agent
    /// profile flows through the exact same argv/cwd/env-shaped spawn call
    /// the first one always did, never a branch on which profile it is.
    pub(crate) fn acp_resolve_agent_launch(
        &self,
    ) -> (
        Vec<String>,
        std::path::PathBuf,
        Vec<(String, String)>,
        String,
    ) {
        if let Some(profile) = self.acp_active_agent_profile() {
            let argv = crate::core::acp::parse_agent_command(&profile.command);
            let cwd = if profile.cwd.trim().is_empty() {
                self.acp_workspace_cwd()
            } else {
                std::path::PathBuf::from(profile.cwd.trim())
            };
            let env = crate::core::acp::parse_agent_env(&profile.env);
            (argv, cwd, env, profile.command.clone())
        } else {
            let cmd = self.settings.acp_agent_command.clone();
            let argv = crate::core::acp::parse_agent_command(&cmd);
            (argv, self.acp_workspace_cwd(), Vec::new(), cmd)
        }
    }

    /// Human-readable summary of `settings.acp_agents` and which is
    /// active, for `:AiAgent` with no argument — same shape as
    /// `acp_mode_status_line` above.
    pub(crate) fn acp_agent_registry_status_line(&self) -> String {
        if self.settings.acp_agents.is_empty() {
            return "No ACP agents configured (settings.acp_agents)".to_string();
        }
        let active = self.acp_active_agent_name();
        let names: Vec<String> = self
            .settings
            .acp_agents
            .iter()
            .map(|a| {
                if a.name.eq_ignore_ascii_case(&active) {
                    format!("*{}", a.name)
                } else {
                    a.name.clone()
                }
            })
            .collect();
        format!("Agents: {}", names.join(", "))
    }

    /// Switch the active agent to `target` (matched case-insensitively
    /// against `settings.acp_agents[].name`), for `:AiAgent <target>`.
    /// Takes effect on the *next* message — the next `ai_send_message`
    /// call spawns the newly-active profile, per #958's "no restart
    /// required" acceptance bar. Ends whatever session is currently live
    /// via `ai_clear` first: a different agent process shares no context
    /// with the old one, so leaving the old transcript on screen next to a
    /// new agent's replies would be actively misleading — same reasoning
    /// `:AiClear` already documents for its own transcript wipe.
    pub(crate) fn acp_switch_agent(&mut self, target: &str) {
        let Some(profile) = self
            .settings
            .acp_agents
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(target))
        else {
            self.message = format!("Unknown ACP agent: {target}");
            return;
        };
        let name = profile.name.clone();
        if name.eq_ignore_ascii_case(&self.acp_active_agent_name()) && self.acp_client.is_none() {
            self.message = format!("Already using agent \"{name}\"");
            return;
        }
        self.ai_clear();
        self.settings.acp_active_agent = name.clone();
        self.message =
            format!("Switched to agent \"{name}\" \u{2014} starts fresh on next message");
    }

    // ── session history / resume (#1459) ────────────────────────────────────

    /// `:AiSessions` entry point. Lists past sessions for the active agent
    /// and workspace from the local index (`acp_session_index` — ACP has no
    /// `session/list` method, see that type's module doc). Refuses outright
    /// — no picker opens — when the agent's most recently learned
    /// `agentCapabilities.loadSession` is `false`: attempting `session/load`
    /// against such an agent would just fail on the wire, so there is
    /// nothing useful to pick from (this issue's acceptance bar: "says so
    /// and does nothing else"). `None` (never learned — this agent has
    /// never been `initialize`d, in this run or a previous one) is treated
    /// the same as "supported": the picker still opens (possibly empty),
    /// and an actual unsupported `session/load` would surface generically
    /// via `AcpEvent::RequestFailed` like any other failed request.
    pub fn acp_open_sessions_picker(&mut self) {
        let agent_name = self.acp_active_agent_name();
        if self.acp_session_index.load_session_capability(&agent_name) == Some(false) {
            self.message = format!(
                "Agent \"{agent_name}\" does not support resuming sessions \
                 (loadSession not advertised)"
            );
            return;
        }
        self.open_picker(PickerSource::AcpSessions);
    }

    /// Resume a past session by id (#1459) — the `:AiSessions` picker's
    /// confirm action. Handles every state `acp_client` can be in by
    /// funnelling all three through the same [`Self::acp_begin_session`]
    /// that already implements the resume-vs-fresh branch and the
    /// "`acp_session_id` set before the request goes out" ordering
    /// guarantee, rather than duplicating that logic here:
    /// - No client at all: spawn + `initialize` one (the same launch path
    ///   `ai_send_message_via_acp`'s cold start uses), then let the
    ///   `Initialized` handler's call to `acp_begin_session` pick up
    ///   `acp_pending_resume` once the handshake completes.
    /// - A client whose handshake is already in flight (a message sent
    ///   moments ago is still waiting on its own `session/new`): same
    ///   deferral, `acp_begin_session` hasn't run for it yet either.
    /// - A live session already: call `acp_begin_session` immediately —
    ///   there is no handshake left to wait for.
    ///
    /// #1459 review: every branch first cancels whatever turn might still
    /// be streaming on the *current* session and clears the displayed
    /// transcript — see [`Self::acp_reset_transcript_for_resume`] — so the
    /// resumed session's replayed history starts from a blank transcript
    /// rather than being spliced onto whatever was already on screen.
    pub(crate) fn acp_resume_session(&mut self, session_id: String) {
        self.acp_cancel_turn();
        self.acp_reset_transcript_for_resume();
        // #1459 review: mark busy from the moment the resume is requested
        // — `AcpEvent::SessionLoaded`'s "no pending prompt" branch is the
        // one thing that turns this back off, whichever of the branches
        // below gets there; leaving it `false` in between would show the
        // panel as idle while a `session/load` round trip is genuinely in
        // flight.
        self.ai_streaming = true;

        if self.acp_client.is_some() && self.acp_session_id.is_some() {
            self.acp_pending_resume = Some(session_id);
            self.acp_begin_session();
            return;
        }
        if self.acp_client.is_some() {
            self.acp_pending_resume = Some(session_id);
            return;
        }
        self.acp_pending_resume = Some(session_id);
        let (argv, cwd, env, agent_label) = self.acp_resolve_agent_launch();
        let env_refs: Vec<(&str, &str)> =
            env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        match crate::core::acp::AcpClient::spawn_with_env(&argv, &cwd, &env_refs) {
            Ok(mut client) => {
                client.initialize();
                self.acp_client = Some(client);
            }
            Err(e) => {
                self.acp_pending_resume = None;
                self.ai_streaming = false;
                self.message = format!("Could not start ACP agent \"{agent_label}\": {e}");
            }
        }
    }

    /// Session-scoped transcript/UI state that must not survive switching
    /// to a different session (#1459 review) — shared by every branch of
    /// [`Self::acp_resume_session`] so the resumed session's `session/
    /// update` replay always lands on a blank transcript, never spliced
    /// onto whatever conversation (if any) was already displayed. Before
    /// this existed, every resume test happened to call `:AiClear`
    /// immediately beforehand, which cleared this same state only as a
    /// side effect of killing the client entirely — masking the "resume a
    /// *different* session while the current one is still live" path,
    /// which never cleared anything.
    ///
    /// Deliberately leaves the connection itself untouched
    /// (`acp_client`/`acp_session_id`/`acp_authenticated`/
    /// `acp_prompt_capabilities`/`acp_auth_methods`) — those describe the
    /// live agent process, which a same-agent resume reuses rather than
    /// replaces, unlike `Engine::ai_clear`'s superset of this reset which
    /// also drops the client.
    ///
    /// `pub(crate)` so `ext_panel.rs`'s `ai_send_message_via_acp` (the
    /// `acp_reopen_last_session` auto-resume path) can call it directly —
    /// the other call site is in this same module.
    pub(crate) fn acp_reset_transcript_for_resume(&mut self) {
        self.acp_remembered_decisions.clear();
        self.ai_messages.clear();
        self.acp_plan.clear();
        self.acp_available_commands.clear();
        self.acp_command_completion_idx = 0;
        self.acp_modes.clear();
        self.acp_current_mode_id = None;
        self.acp_usage = None;
        self.acp_tool_calls.clear();
        self.change_review = None;
        self.ai_chat.borrow_mut().set_transcript_scroll_top(0);
    }

    // ── authMethods / authenticate / terminal login (#957, ACP-6) ───────────

    /// Send `session/new` for the live `acp_client` — the second half of
    /// the handshake, run either immediately after `Initialized` (no auth
    /// required) or after auth resolves (`AcpEvent::Authenticated`, the
    /// `"acp_auth_choice"` dialog's skip button, or a successful terminal
    /// login). Factored out of the `Initialized` handler so all four call
    /// sites share it instead of re-deriving the same two lines.
    ///
    /// `pub(crate)` so `panels.rs`'s `"acp_auth_choice"` dialog arm (the
    /// "Continue without auth" / cancel path, which produces no
    /// `AcpEvent`) can call it directly — the other three call sites are
    /// all in this same module.
    ///
    /// #1459: if [`Self::acp_pending_resume`] holds a session id (set by
    /// `:AiSessions`'s resume action, or the `acp_reopen_last_session`
    /// setting's "resume on first `:AI`" path), sends `session/load` for it
    /// instead of `session/new` — folded into this one shared function so
    /// all four call sites get resume support for free, exactly as they
    /// already share the fresh-session path. `acp_session_id` is set to the
    /// resumed id *before* the request goes out, not from the eventual
    /// `AcpEvent::SessionLoaded` response: a resuming agent may replay the
    /// session's history as `session/update` notifications before that
    /// response line arrives, and `SessionUpdate`'s handler only accepts
    /// updates for whatever `acp_session_id` already is.
    pub(crate) fn acp_begin_session(&mut self) {
        let cwd = self.acp_workspace_cwd();
        if let Some(session_id) = self.acp_pending_resume.take() {
            self.acp_session_id = Some(session_id.clone());
            self.message = format!("Resuming session {session_id}\u{2026}");
            if let Some(client) = self.acp_client.as_mut() {
                client.load_session(&session_id, &cwd, vec![]);
            }
            return;
        }
        if let Some(client) = self.acp_client.as_mut() {
            client.new_session(&cwd, vec![]);
        }
    }

    /// Open the `"acp_auth_choice"` dialog listing `self.acp_auth_methods`
    /// plus a "Continue without auth" fallback — an agent advertising
    /// `authMethods` doesn't necessarily mean auth is *required* right now
    /// (it may already be logged in from a previous run), so the human can
    /// always decline and let `session/new` itself succeed or fail. Button
    /// `action` is the chosen method's `id` (looked back up against
    /// `acp_auth_methods` by `"acp_auth_choice"`'s `process_dialog_result`
    /// arm in `panels.rs`), or `"acp_auth_skip"` for the fallback — same
    /// "hotkey scanned left-to-right, first free letter wins" collision
    /// avoidance `acp_handle_permission_request` already uses, extended to
    /// cover the fallback button too so it can never silently steal a
    /// method's letter or vice versa.
    fn acp_show_auth_choice(&mut self) {
        let mut labeled_actions: Vec<(String, String)> = self
            .acp_auth_methods
            .iter()
            .map(|m| (m.name.clone(), m.id.clone()))
            .collect();
        labeled_actions.push((
            "Continue without auth".to_string(),
            "acp_auth_skip".to_string(),
        ));

        let mut used_hotkeys: std::collections::HashSet<char> = std::collections::HashSet::new();
        let buttons: Vec<DialogButton> = labeled_actions
            .into_iter()
            .map(|(label, action)| {
                let hotkey = label
                    .chars()
                    .map(|c| c.to_ascii_lowercase())
                    .find(|c| c.is_ascii_alphabetic() && !used_hotkeys.contains(c))
                    .unwrap_or('\0');
                if hotkey != '\0' {
                    used_hotkeys.insert(hotkey);
                }
                DialogButton {
                    label,
                    hotkey,
                    action,
                }
            })
            .collect();

        self.show_dialog(
            "acp_auth_choice",
            "Authenticate",
            vec!["This agent offers the following ways to sign in:".to_string()],
            buttons,
        );
    }

    /// Called once an ACP terminal-auth login pane is gone — either it
    /// exited on its own (`poll_terminal`'s exit handling, `exit_code =
    /// Some(_)`) or the human closed it first
    /// (`terminal_close_active_tab`, `exit_code = None`, i.e. abandoned).
    /// `Some(0)` resumes the handshake exactly per the issue's acceptance
    /// bar ("completion re-initializes the session"): re-send `initialize`
    /// on the *same* still-alive `acp_client` — its `Initialized` handler
    /// above will skip the dialog this time (`acp_authenticated` is now
    /// `true`) and proceed straight to `session/new`. Anything else
    /// (non-zero exit, or abandoned) must leave the panel usable per the
    /// issue's other acceptance bar, not wedged: drop `acp_client`
    /// entirely so the next `:AI`/panel message starts a clean
    /// spawn+initialize+dialog rather than silently queuing a prompt
    /// against a client that will never send another `session/new`.
    pub(crate) fn acp_finish_terminal_login(&mut self, exit_code: Option<u32>) {
        if exit_code == Some(0) {
            self.acp_authenticated = true;
            self.message = "Sign-in complete, resuming\u{2026}".to_string();
            if let Some(client) = self.acp_client.as_mut() {
                client.initialize();
            }
            return;
        }

        let detail = match exit_code {
            Some(code) => format!("exited with code {code}"),
            None => "was abandoned".to_string(),
        };
        self.message = format!("ACP sign-in {detail} \u{2014} try again from the AI panel");
        self.ai_messages.push(AiMessage {
            role: "assistant-thought".to_string(),
            content: format!("\u{26a0} Sign-in {detail}."),
        });
        self.ai_streaming = false;
        self.acp_streaming_turn = None;
        self.acp_pending_prompt = None;
        // #1459: same reasoning as `AgentExited` — an abandoned/failed
        // login drops the client entirely, so any deferred resume display
        // line has nowhere left to land.
        self.acp_pending_prompt_display = None;
        self.acp_pending_resume = None;
        self.acp_client = None;
        self.acp_session_id = None;
        self.acp_auth_methods.clear();
        self.acp_authenticated = false;
        self.acp_prompt_capabilities = crate::core::acp::AcpPromptCapabilities::default();
    }

    // ── fs/read_text_file, fs/write_text_file (#954, ACP-3) ─────────────────

    /// Directories an ACP agent's `fs/write_text_file` may write inside.
    /// Today that's just the session `cwd` — ACP's `session/new` has an
    /// `additionalDirectories` field in later protocol drafts, but this
    /// client's `session/new` (`AcpClient::new_session`) doesn't negotiate
    /// it yet, so there is nothing else to add here. Kept as its own
    /// `Vec`-returning method (rather than inlining `vec![self.
    /// acp_workspace_cwd()]` at each call site) so wiring up
    /// `additionalDirectories` later is a one-line change in one place.
    fn acp_workspace_roots(&self) -> Vec<std::path::PathBuf> {
        vec![self.acp_workspace_cwd()]
    }

    // ── prompt context: current buffer + `@`-mentions (#1449) ───────────────

    /// The active buffer, resolved into an ACP `resource_link` content
    /// block plus a short "what got attached" chip line for the
    /// transcript (`⧉ <path relative to the workspace>`) — `None` when
    /// `settings.ai_attach_current_buffer` is off, or the active buffer
    /// has no path (an unnamed/scratch buffer has nothing on disk to
    /// link), or the path can't be resolved inside the workspace roots
    /// (same defensive check `fs/read_text_file`/`fs/write_text_file` use,
    /// via [`crate::core::acp::resolve_path_within_roots`]).
    ///
    /// Shared by [`Self::ai_send_message_via_acp`] (the chip, so the
    /// *displayed* message names what's attached) and
    /// [`Self::acp_prompt_content_blocks`] (the actual wire content) so
    /// the two can never disagree about what got attached.
    pub(crate) fn acp_current_buffer_attachment(&self) -> Option<(serde_json::Value, String)> {
        if !self.settings.ai_attach_current_buffer {
            return None;
        }
        let path = self.active_buffer_state().file_path.clone()?;
        let roots = self.acp_workspace_roots();
        let resolved = crate::core::acp::resolve_path_within_roots(&path, &roots).ok()?;
        let display =
            crate::core::acp::workspace_relative_display(&resolved, &self.acp_workspace_cwd());
        let name = resolved
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| display.clone());
        let block = serde_json::json!({
            "type": "resource_link",
            "uri": crate::core::lsp::path_to_uri(&resolved),
            "name": name,
        });
        Some((block, format!("\u{29c9} {display}")))
    }

    /// Resolve one `@`-mention query (the raw text after `@`, as typed —
    /// relative paths are joined onto the first workspace root, same
    /// convention `fs/read_text_file` uses) into a `resource_link` content
    /// block, or `None` if it doesn't resolve to a real path inside the
    /// workspace. A mention of a file that doesn't exist, or that resolves
    /// outside the workspace, is silently dropped from the wire content —
    /// the literal `@path` text the user typed stays in the message either
    /// way (see [`Self::acp_prompt_content_blocks`]).
    fn acp_mention_resource_link(&self, mention: &str) -> Option<serde_json::Value> {
        let roots = self.acp_workspace_roots();
        let root = roots.first().cloned().unwrap_or_else(|| self.cwd.clone());
        let candidate = root.join(mention);
        let resolved = crate::core::acp::resolve_path_within_roots(&candidate, &roots).ok()?;
        let name = resolved
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| mention.to_string());
        Some(serde_json::json!({
            "type": "resource_link",
            "uri": crate::core::lsp::path_to_uri(&resolved),
            "name": name,
        }))
    }

    /// Build the ACP `session/prompt` content-block array for `text`
    /// (#1449): the typed text verbatim as a `{"type": "text"}` block —
    /// including any literal `@path` mentions, untouched — plus a
    /// `resource_link` for the active buffer
    /// ([`Self::acp_current_buffer_attachment`]) and one more for every
    /// `@`-mention in `text` that resolves to a real workspace path
    /// ([`Self::acp_mention_resource_link`]). `resource_link` is baseline
    /// ACP v1 — every agent must accept it, no `promptCapabilities` check
    /// needed (unlike the embedded-context block below).
    ///
    /// #1450: also consumes `self.acp_pending_attachment` (the Visual
    /// selection / `:{range}AI` attachment), if one is staged, into its own
    /// content block(s) via [`crate::core::acp::AcpRangeAttachment::
    /// content_blocks`] — the one call in this codebase that reads
    /// `self.acp_prompt_capabilities` to choose `resource` vs.
    /// `resource_link` + fenced text. `&mut self` (not `&self`, unlike
    /// #1449's original signature) purely for the `.take()` — the
    /// attachment is consumed exactly once, whichever of this function's
    /// two call sites ends up sending it (immediately in
    /// `Self::ai_send_message_via_acp`, or after the handshake completes in
    /// `Self::poll_acp`'s `SessionCreated` arm).
    pub(crate) fn acp_prompt_content_blocks(&mut self, text: &str) -> Vec<serde_json::Value> {
        let mut blocks = vec![serde_json::json!({"type": "text", "text": text})];
        if let Some((block, _chip)) = self.acp_current_buffer_attachment() {
            blocks.push(block);
        }
        for mention in crate::core::acp::parse_at_mentions(text) {
            if let Some(block) = self.acp_mention_resource_link(&mention) {
                blocks.push(block);
            }
        }
        if let Some(attachment) = self.acp_pending_attachment.take() {
            blocks.extend(attachment.content_blocks(self.acp_prompt_capabilities));
        }
        // #1464: every manually attached file/image, in the order they were
        // attached — `drain(..)` so a removed-before-send attachment (Ctrl+R)
        // never reaches this point in the first place, and a sent one is
        // gone for the *next* prompt, same one-shot-per-send contract
        // `acp_pending_attachment.take()` above already has.
        for attachment in self.acp_manual_attachments.drain(..) {
            blocks.push(attachment.content_block());
        }
        blocks
    }

    // ── prompt context: manual file/image attachments (#1464) ───────────────

    /// `:AiAttach <path>` — stage `path` as the next prompt's attachment.
    /// `path` is resolved the same way `Self::acp_current_buffer_attachment`
    /// resolves the active buffer's path: joined onto the first workspace
    /// root when relative, then must land inside a workspace root (see
    /// [`crate::core::acp::resolve_path_within_roots`]) — a path outside the
    /// workspace is refused with a clear message, not silently dropped.
    ///
    /// An image extension ([`crate::core::acp::image_mime_type_for_path`])
    /// stages an `image` content block instead of a `resource_link` — gated
    /// on `self.acp_prompt_capabilities.image` (refused with a clear message
    /// when the agent hasn't declared support — there is no baseline-ACP
    /// fallback for binary image data) and on
    /// [`crate::core::acp::ACP_MAX_IMAGE_ATTACHMENT_BYTES`] (refused the
    /// same way). Anything else attaches as a baseline `resource_link` —
    /// no capability check, no size limit, since nothing is read off disk
    /// for that variant.
    pub(crate) fn acp_attach_file(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() {
            self.message = "Usage: :AiAttach <path>".to_string();
            return;
        }
        let roots = self.acp_workspace_roots();
        let root = roots.first().cloned().unwrap_or_else(|| self.cwd.clone());
        let candidate = root.join(arg);
        let resolved = match crate::core::acp::resolve_path_within_roots(&candidate, &roots) {
            Ok(p) => p,
            Err(_) => {
                self.message = format!("Cannot attach '{arg}': outside the workspace");
                return;
            }
        };
        if !resolved.is_file() {
            self.message = format!("Cannot attach '{arg}': no such file");
            return;
        }

        if let Some(mime_type) = crate::core::acp::image_mime_type_for_path(&resolved) {
            if !self.acp_prompt_capabilities.image {
                self.message = "This agent doesn't support image attachments \
                    (promptCapabilities.image is false)"
                    .to_string();
                return;
            }
            let size = std::fs::metadata(&resolved).map(|m| m.len()).unwrap_or(0);
            if size > crate::core::acp::ACP_MAX_IMAGE_ATTACHMENT_BYTES {
                self.message = format!(
                    "Cannot attach '{arg}': {} is larger than the {} limit",
                    crate::core::acp::format_byte_size(size),
                    crate::core::acp::format_byte_size(
                        crate::core::acp::ACP_MAX_IMAGE_ATTACHMENT_BYTES
                    ),
                );
                return;
            }
            let data = match std::fs::read(&resolved) {
                Ok(d) => d,
                Err(e) => {
                    self.message = format!("Cannot attach '{arg}': {e}");
                    return;
                }
            };
            let name = resolved
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| arg.to_string());
            self.acp_manual_attachments
                .push(crate::core::acp::AcpManualAttachment::Image {
                    name,
                    mime_type: mime_type.to_string(),
                    data,
                });
            self.message = format!("Attached image '{arg}'");
        } else {
            self.acp_manual_attachments
                .push(crate::core::acp::AcpManualAttachment::File { path: resolved });
            self.message = format!("Attached '{arg}'");
        }
    }

    /// Paste an image straight off the system clipboard as the next
    /// prompt's attachment (#1464) — the "paste an image" half of the
    /// issue, alongside `:AiAttach`'s "attach by path" half.
    /// `self.clipboard_read_image` is the callback the GTK backend wires at
    /// startup (`App::setup_gtk_clipboard`'s image twin), straight through
    /// to `quadraui::Clipboard::read_image`; TUI never wires one (no
    /// terminal clipboard-image channel — see that callback's own doc), so
    /// this is a clear "can't paste an image here" message there, never a
    /// panic or a silent no-op. Same `promptCapabilities.image` gate and
    /// [`crate::core::acp::ACP_MAX_IMAGE_ATTACHMENT_BYTES`] size limit as
    /// `Self::acp_attach_file`'s image branch — checked against the
    /// *encoded* PNG size, since that's what actually goes over the wire.
    pub(crate) fn acp_attach_clipboard_image(&mut self) {
        let Some(read_image) = self.clipboard_read_image.as_ref() else {
            self.message = "This platform can't paste an image from the clipboard".to_string();
            return;
        };
        if !self.acp_prompt_capabilities.image {
            self.message = "This agent doesn't support image attachments \
                (promptCapabilities.image is false)"
                .to_string();
            return;
        }
        let image = match read_image() {
            Ok(img) => img,
            Err(quadraui::BackendError::Unsupported) => {
                self.message = "This platform can't paste an image from the clipboard".to_string();
                return;
            }
            Err(e) => {
                self.message = format!("Clipboard paste failed: {e:?}");
                return;
            }
        };
        let png_bytes =
            match crate::core::acp::encode_png_rgba8(image.width, image.height, &image.pixels) {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.message = format!("Could not encode clipboard image: {e}");
                    return;
                }
            };
        if png_bytes.len() as u64 > crate::core::acp::ACP_MAX_IMAGE_ATTACHMENT_BYTES {
            self.message = format!(
                "Clipboard image is {} \u{2014} larger than the {} limit",
                crate::core::acp::format_byte_size(png_bytes.len() as u64),
                crate::core::acp::format_byte_size(
                    crate::core::acp::ACP_MAX_IMAGE_ATTACHMENT_BYTES
                ),
            );
            return;
        }
        self.acp_manual_attachments
            .push(crate::core::acp::AcpManualAttachment::Image {
                name: "(pasted image)".to_string(),
                mime_type: "image/png".to_string(),
                data: png_bytes,
            });
        self.message = "Attached clipboard image".to_string();
    }

    // ── prompt context: Visual selection / `:{range}AI` (#1450) ─────────────

    /// Build an [`crate::core::acp::AcpRangeAttachment`] for lines
    /// `[start_line, end_line]` (0-based, inclusive) of the active buffer.
    /// `exact_text`, when given, overrides the whole-lines default with the
    /// precise selected text — the Visual mapping's characterwise/blockwise
    /// case (point 5: "send the exact selected text; the URI range still
    /// names whole lines"). `None` when there's no file to link to (mirrors
    /// [`Self::acp_current_buffer_attachment`]'s unnamed-buffer/outside-
    /// workspace bail-outs) — a scratch buffer has nothing on disk an agent
    /// could resolve the URI against.
    pub(crate) fn acp_build_range_attachment(
        &self,
        start_line: usize,
        end_line: usize,
        exact_text: Option<String>,
    ) -> Option<crate::core::acp::AcpRangeAttachment> {
        let path = self.active_buffer_state().file_path.clone()?;
        let roots = self.acp_workspace_roots();
        let resolved = crate::core::acp::resolve_path_within_roots(&path, &roots).ok()?;
        let (start_line, end_line) = (start_line.min(end_line), start_line.max(end_line));
        let text = match exact_text {
            Some(t) => t,
            None => {
                let last = self.buffer().len_lines().saturating_sub(1);
                let end_line = end_line.min(last);
                let start_char = self.buffer().line_to_char(start_line.min(end_line));
                let end_char = if end_line + 1 < self.buffer().len_lines() {
                    self.buffer().line_to_char(end_line + 1)
                } else {
                    self.buffer().len_chars()
                };
                self.buffer()
                    .content
                    .slice(start_char..end_char)
                    .to_string()
            }
        };
        Some(crate::core::acp::AcpRangeAttachment {
            path: resolved,
            start_line,
            end_line,
            text,
        })
    }

    /// `:{range}AI [message]` (#1450 point 1) — stage lines `[start_line,
    /// end_line]` of the current buffer as the next attachment, then either
    /// send `message` immediately (mirroring plain `:AI <message>`) or, if
    /// `message` is empty (`:'<,'>AI` alone), just focus the panel so the
    /// user can type one — the attachment stays staged either way, so it's
    /// not lost by typing the message separately.
    ///
    /// The two cases differ in *keyboard* focus, deliberately (#958
    /// regression): with a message the user handed over a complete ex
    /// command and stays where they were, so this only reveals the panel
    /// (`ai_has_focus`, exactly what plain `:AI <message>` has always done)
    /// and must **not** pull the keyboard into the chat input — otherwise
    /// the next `:` typed after `:AI hi` becomes chat text instead of
    /// opening the command line, and consecutive ex commands (`:AI hi` then
    /// `:AiAgent beta`) stop working. With no message there is nothing to
    /// send until the user types one, so the input does need the keyboard.
    pub(crate) fn ai_attach_range(&mut self, start_line: usize, end_line: usize, message: &str) {
        if let Some(attachment) = self.acp_build_range_attachment(start_line, end_line, None) {
            self.acp_pending_attachment = Some(attachment);
        }
        if message.is_empty() {
            self.acp_focus_ai_panel_for_keyboard();
        } else {
            self.ai_send_message(message.to_string());
            self.focus_sidebar_panel(crate::core::engine::sidebar::PANEL_AI);
        }
    }

    /// `<leader>ai` in Visual mode (#1450 point 2): stage the current
    /// selection as the next attachment (exact text for characterwise/
    /// blockwise, whole lines for linewise — point 5), exit Visual mode,
    /// and focus the AI panel with the cursor in its input — without
    /// sending; the user still types and submits a message. Outside Visual
    /// mode, or with nothing to attach to (no active selection, or the
    /// active buffer has no file), this just focuses the panel — the same
    /// "no attachment, just open the panel" fallback `chat_open` already
    /// has.
    pub(crate) fn acp_attach_visual_selection_and_focus(&mut self) {
        if let Some((start, end)) = self.get_visual_selection_range() {
            let linewise = matches!(self.mode, Mode::VisualLine);
            let exact_text = if linewise {
                None
            } else {
                self.get_visual_selection_text().map(|(t, _)| t)
            };
            if let Some(attachment) =
                self.acp_build_range_attachment(start.line, end.line, exact_text)
            {
                self.acp_pending_attachment = Some(attachment);
            }
            self.mode = Mode::Normal;
            self.visual_anchor = None;
            self.visual_dollar = false;
            self.count = None;
        }
        self.acp_focus_ai_panel_for_keyboard();
    }

    /// Shared by [`Self::ai_attach_range`] and
    /// [`Self::acp_attach_visual_selection_and_focus`]: a genuine
    /// programmatic reveal, not a bare `self.ai_has_focus = true` — the AI
    /// panel may not already be the visible sidebar content, and
    /// `render::sidebar_owner`/`Engine::active_panel_is` (what actually
    /// decides what paints, on both backends) need `app_shell.show_panel` to
    /// have run for it to show up at all, per [`Self::focus_sidebar_panel`]'s
    /// own doc ("used for programmatic reveals like DAP session start").
    ///
    /// `focus_sidebar_panel` alone doesn't put keyboard input in the panel's
    /// input on TUI: `sidebar.has_focus` there is a cached copy of "is the
    /// sidebar band focused", updated ad hoc by mouse/shell-event handlers,
    /// not re-derived every keystroke the way GTK's
    /// `Engine::sidebar_has_focus` call is (see that method's doc) — so this
    /// also raises the one-shot [`Engine::sidebar_focus_requested`] flag,
    /// which `render::post_key_epilogue` drains into
    /// `PostKeyEpilogue::focus_sidebar` for whichever backend is running. A
    /// one-shot request, not a "whenever `ai_has_focus` is set" rule: the
    /// latter also fires on the keypresses *after* the reveal, which is what
    /// broke `:AI hi` followed by `:AiAgent beta` (#958) — see
    /// [`Self::ai_attach_range`].
    fn acp_focus_ai_panel_for_keyboard(&mut self) {
        self.focus_sidebar_panel(crate::core::engine::sidebar::PANEL_AI);
        self.sidebar_focus_requested = true;
    }

    /// Answer a parked `fs/read_text_file` request. A malformed request
    /// (missing `sessionId`/`path`) gets a JSON-RPC error, never silence —
    /// same policy as the malformed `session/request_permission` path
    /// above.
    fn acp_handle_read_text_file(&mut self, request_id: i64, params: serde_json::Value) {
        let Some(req) = crate::core::acp::parse_read_text_file_params(&params) else {
            self.acp_respond_error(request_id, "invalid fs/read_text_file params");
            return;
        };
        match self.acp_read_text_file(std::path::Path::new(&req.path), req.line, req.limit) {
            Ok(content) => {
                if let Some(client) = self.acp_client.as_ref() {
                    client.respond_to_client_request(
                        request_id,
                        Ok(crate::core::acp::read_text_file_result(&content)),
                    );
                }
            }
            Err(msg) => self.acp_respond_error(request_id, &msg),
        }
    }

    /// Read `path`'s text content for `fs/read_text_file`. **Buffer-first**:
    /// an already-open buffer's in-memory content — including unsaved edits
    /// — wins over whatever is on disk. This is the single most important
    /// correctness property in #954's slice: an agent that reads a dirty
    /// buffer from disk reasons about stale text and proposes edits against
    /// lines the user already changed. Falls back to the filesystem only
    /// when no buffer has this path open. `line`/`limit` are applied via
    /// [`crate::core::acp::select_text_lines`] regardless of which source
    /// served the content.
    pub(crate) fn acp_read_text_file(
        &self,
        path: &Path,
        line: Option<u32>,
        limit: Option<u32>,
    ) -> Result<String, String> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let buffer_text = self.buffer_manager.iter().find_map(|(_, state)| {
            let existing = state.file_path.as_ref()?;
            let existing_canonical = existing.canonicalize().unwrap_or_else(|_| existing.clone());
            (existing_canonical == canonical).then(|| state.buffer.content.to_string())
        });
        let text = match buffer_text {
            Some(t) => t,
            None => std::fs::read_to_string(path)
                .map_err(|e| format!("failed to read {}: {e}", path.display()))?,
        };
        Ok(crate::core::acp::select_text_lines(&text, line, limit))
    }

    /// Answer a parked `fs/write_text_file` request. A malformed request
    /// gets a JSON-RPC error; a well-formed one that fails for another
    /// reason (path outside the workspace, disk write error, ...) gets its
    /// message surfaced as a JSON-RPC error too — never a swallowed
    /// failure the way the pre-#954 closed-file `apply_workspace_edit`
    /// branch was.
    fn acp_handle_write_text_file(&mut self, request_id: i64, params: serde_json::Value) {
        let Some(req) = crate::core::acp::parse_write_text_file_params(&params) else {
            self.acp_respond_error(request_id, "invalid fs/write_text_file params");
            return;
        };
        match self.acp_write_text_file(std::path::Path::new(&req.path), &req.content) {
            Ok(()) => {
                if let Some(client) = self.acp_client.as_ref() {
                    client.respond_to_client_request(request_id, Ok(serde_json::Value::Null));
                }
            }
            Err(msg) => self.acp_respond_error(request_id, &msg),
        }
    }

    /// Serve `fs/write_text_file`: open (or reuse) a buffer for `path`,
    /// replace its whole content through the undo-grouped path (a single
    /// `u` reverts the whole write, like any other edit), then persist to
    /// disk — creating the file if it didn't already exist, per the ACP v1
    /// spec's requirement for this method. Refuses to write outside
    /// [`Self::acp_workspace_roots`].
    ///
    /// This exists specifically so ACP's write path never falls into the
    /// bug `apply_workspace_edit`'s old closed-file branch had (no undo, no
    /// canonicalisation, swallowed errors) — see that method's doc comment
    /// in `panels.rs` for the fuller history. Unlike `apply_workspace_edit`
    /// (which leaves a closed-file edit dirty for the user to review/save,
    /// matching how an edit to an already-open buffer behaves),
    /// `fs/write_text_file` is inherently a disk write — the whole point of
    /// the RPC — so this persists immediately rather than leaving the write
    /// invisible to anything reading the file outside the editor (a shell
    /// command the same agent runs next, say).
    pub(crate) fn acp_write_text_file(&mut self, path: &Path, content: &str) -> Result<(), String> {
        let roots = self.acp_workspace_roots();
        let resolved = crate::core::acp::resolve_path_within_roots(path, &roots)?;
        let buffer_id = self
            .buffer_manager
            .open_file(&resolved)
            .map_err(|e| format!("failed to open {}: {e}", resolved.display()))?;
        self.acp_replace_buffer_content(buffer_id, content);
        self.save_buffer_by_id(buffer_id)
    }

    /// Replace the entirety of `buffer_id`'s content with `new_content` as
    /// one undo-grouped edit — the same start/finish-undo-group shape
    /// `apply_lsp_edits` uses, just for a whole-buffer swap instead of a
    /// list of ranged edits (there is no LSP-style range list to apply for
    /// `fs/write_text_file`; the agent hands over the whole new file body).
    fn acp_replace_buffer_content(&mut self, buffer_id: BufferId, new_content: &str) {
        let cursor = self
            .windows
            .values()
            .find(|w| w.buffer_id == buffer_id)
            .map(|w| w.view.cursor)
            .unwrap_or_default();
        let Some(state) = self.buffer_manager.get_mut(buffer_id) else {
            return;
        };
        state.start_undo_group(cursor);
        let old_len = state.buffer.content.len_chars();
        if old_len > 0 {
            let deleted: String = state.buffer.content.slice(0..old_len).chars().collect();
            state.buffer.content.remove(0..old_len);
            state.record_delete(0, &deleted);
        }
        if !new_content.is_empty() {
            state.buffer.content.insert(0, new_content);
            state.record_insert(0, new_content);
        }
        state.dirty = true;
        state.finish_undo_group(cursor);
        state.semantic_tokens.clear();
        self.lsp_dirty_buffers.insert(buffer_id, true);
    }

    /// Reply to a parked agent -> client request with a JSON-RPC error —
    /// the shared "surface it, never swallow it" tail every `fs/*` handler
    /// above funnels through.
    fn acp_respond_error(&self, request_id: i64, message: &str) {
        if let Some(client) = self.acp_client.as_ref() {
            client.respond_to_client_request(request_id, Err((-32000, message.to_string())));
        }
    }

    /// Append one `session/update` chunk to the AI panel transcript
    /// (`self.ai_messages`), appending to the in-progress streamed turn
    /// when `kind` matches it and starting a new turn otherwise — the
    /// "streamed assistant turn" ACP-1 asks for rather than one message
    /// per chunk. Empty chunks (a still-loading tool-adjacent update with
    /// no text yet) are dropped rather than starting a spurious turn.
    fn acp_append_chunk(&mut self, kind: AcpChunkKind, text: String) {
        if text.is_empty() {
            return;
        }
        if let Some((idx, streaming_kind)) = self.acp_streaming_turn {
            if streaming_kind == kind && idx < self.ai_messages.len() {
                self.ai_messages[idx].content.push_str(&text);
                return;
            }
        }
        let role = match kind {
            AcpChunkKind::Message => "assistant",
            // Kept visually distinct from `Message` in
            // `render::populate_ai_chat_controller` (renders as
            // `quadraui::ChatRole::System`) — ACP-1's acceptance
            // criteria: thought chunks must not merge into message text.
            AcpChunkKind::Thought => "assistant-thought",
            AcpChunkKind::UserEcho => "user",
        };
        self.ai_messages.push(AiMessage {
            role: role.to_string(),
            content: text,
        });
        self.acp_streaming_turn = Some((self.ai_messages.len() - 1, kind));
    }

    // ── plan, available_commands_update, current_mode_update, usage_update
    //    (#956, ACP-5) ───────────────────────────────────────────────────────

    /// Dispatch one `session/update` notification's inner tagged-union
    /// payload to whichever variant it matches. `session_update_chunk`
    /// (message/thought/user-echo streaming, ACP-1) is tried first since
    /// it's the highest-traffic variant; the rest are each a full
    /// wholesale replacement of the corresponding `Engine` field, never a
    /// delta — most importantly `plan`, whose #956 acceptance bar requires
    /// two successive updates leave exactly one plan rendered.
    /// `tool_call`/`tool_call_update` remain unhandled here (ACP-4's scope).
    fn acp_handle_session_update(&mut self, inner: &serde_json::Value) {
        if let Some((kind, text)) = crate::core::acp::session_update_chunk(inner) {
            self.acp_append_chunk(kind, text);
        } else if let Some(entries) = crate::core::acp::parse_plan_update(inner) {
            self.acp_plan = entries;
        } else if let Some(commands) = crate::core::acp::parse_available_commands_update(inner) {
            self.acp_available_commands = commands;
            self.acp_command_completion_idx = 0;
        } else if let Some(mode_id) = crate::core::acp::parse_current_mode_update(inner) {
            self.acp_current_mode_id = Some(mode_id);
        } else if let Some(usage) = crate::core::acp::parse_usage_update(inner) {
            self.acp_usage = Some(usage);
        } else if let Some(call) = crate::core::acp::parse_tool_call(inner) {
            // #955 (ACP-4).
            self.acp_upsert_tool_call(call);
        } else if let Some(update) = crate::core::acp::parse_tool_call_update(inner) {
            // #955 (ACP-4).
            self.acp_apply_tool_call_update(update);
        }
        // Anything else (an update kind this client doesn't know about
        // yet) is a forward-compatible no-op, same policy ACP-1
        // established for the whole stream.
    }

    // ── tool_call / tool_call_update (#955, ACP-4) ──────────────────────────

    /// Insert or replace `call` in `self.acp_tool_calls`, keyed by
    /// `AcpToolCall::id` — the "addressable collection, not an
    /// append-only log" the issue asks for. Any `diff` content block the
    /// call already carries opens (or extends) the change-review surface
    /// immediately, same as a `tool_call_update` adding one later.
    fn acp_upsert_tool_call(&mut self, call: crate::core::acp::AcpToolCall) {
        self.acp_open_review_for_diffs(&call.content);
        match self.acp_tool_calls.iter_mut().find(|t| t.id == call.id) {
            Some(existing) => *existing = call,
            None => self.acp_tool_calls.push(call),
        }
    }

    /// Apply a `tool_call_update` patch by id: `status`, when present,
    /// replaces the call's status — the `pending -> in_progress ->
    /// completed | failed` transition the issue's acceptance bar checks —
    /// and `content`, when present, is **appended** to the call's
    /// existing content (never replaces it), per `AcpToolCallUpdate`'s
    /// doc. An update for an id this client never saw a `tool_call` for is
    /// a no-op — nothing to patch, and inventing a call from a bare update
    /// would render with an empty title.
    fn acp_apply_tool_call_update(&mut self, update: crate::core::acp::AcpToolCallUpdate) {
        if let Some(blocks) = &update.content {
            self.acp_open_review_for_diffs(blocks);
        }
        let Some(call) = self.acp_tool_calls.iter_mut().find(|t| t.id == update.id) else {
            return;
        };
        if let Some(status) = update.status {
            call.status = status;
        }
        if let Some(mut blocks) = update.content {
            call.content.append(&mut blocks);
        }
    }

    /// Open (or extend) the change-review surface for every `diff` block in
    /// `blocks` — "a `diff` content block opens the change-review surface"
    /// (#955's acceptance bar). Non-diff blocks are ignored here; they're
    /// already stored on the call itself by the caller. Source-agnostic:
    /// builds `crate::core::review::ProposedChange`, the exact shape a
    /// non-ACP feeder (e.g. #525's git-branch diff list) would construct
    /// directly.
    ///
    /// Each block is resolved through [`Self::acp_resolve_diff_block`]
    /// first (#1454) — a block whose `oldText` can't be unambiguously
    /// located in the file's actual current content never reaches
    /// `changes` at all, so a batch with one bad block still opens (or
    /// extends) the surface with whichever others resolved cleanly.
    fn acp_open_review_for_diffs(&mut self, blocks: &[crate::core::acp::AcpToolCallContentBlock]) {
        let mut changes: Vec<crate::core::review::ProposedChange> = Vec::new();
        for block in blocks {
            let crate::core::acp::AcpToolCallContentBlock::Diff {
                path,
                old_text,
                new_text,
            } = block
            else {
                continue;
            };
            if let Some(change) = self.acp_resolve_diff_block(path, old_text.as_deref(), new_text) {
                changes.push(change);
            }
        }
        if changes.is_empty() {
            return;
        }
        match &mut self.change_review {
            Some(review) => review.extend(changes),
            None => self.change_review = Some(crate::core::review::ChangeReviewState::new(changes)),
        }
    }

    /// Resolve one `diff` content block into a whole-file
    /// [`crate::core::review::ProposedChange`] ready for the review
    /// surface, or `None` if it must not open (or extend) one at all
    /// (#1454) — the safety gate between an agent-reported fragment and
    /// ever treating it as if it were the entire file.
    ///
    /// `old_text: None` (a new file, per the ACP v1 schema's `oldText:
    /// string | null`) passes `new_text` straight through unchanged —
    /// there is no "current contents" to resolve a new file against. A
    /// non-null `old_text` is resolved via
    /// [`crate::core::review::resolve_fragment`] against the file's actual
    /// current content (read buffer-first, matching every other ACP
    /// `fs/*` read in this module, via [`Self::acp_current_file_content`]):
    /// an unambiguous single match becomes an `Applied` [`ProposedChange`]
    /// whose `old_text`/`new_text` are both provably whole-file (never the
    /// bare fragment); `AlreadyApplied` (the edit already landed some other
    /// way — the double-apply race #1454 flags for an adapter that applies
    /// edits itself and merely *reports* them via `diff`) and `Refused`
    /// (the fragment doesn't uniquely identify a location) both leave
    /// [`Self::message`](Engine::message) with a human-readable explanation
    /// and return `None` rather than ever opening an entry whose
    /// `new_text` isn't safe to write.
    ///
    /// [`ProposedChange`]: crate::core::review::ProposedChange
    fn acp_resolve_diff_block(
        &mut self,
        path: &str,
        old_text: Option<&str>,
        new_text: &str,
    ) -> Option<crate::core::review::ProposedChange> {
        let Some(fragment) = old_text else {
            return Some(crate::core::review::ProposedChange {
                path: path.to_string(),
                old_text: None,
                new_text: new_text.to_string(),
            });
        };
        let current = match self.acp_current_file_content(std::path::Path::new(path)) {
            Ok(text) => text,
            Err(msg) => {
                self.message = format!(
                    "Change review: could not read {path} to apply the proposed edit ({msg})"
                );
                return None;
            }
        };
        match crate::core::review::resolve_fragment(&current, fragment, new_text) {
            crate::core::review::FragmentResolution::Applied {
                old_whole,
                new_whole,
            } => Some(crate::core::review::ProposedChange {
                path: path.to_string(),
                old_text: Some(old_whole),
                new_text: new_whole,
            }),
            crate::core::review::FragmentResolution::AlreadyApplied => {
                self.message =
                    format!("Change review: {path} edit already applied, nothing to review");
                None
            }
            crate::core::review::FragmentResolution::Refused(reason) => {
                self.message = format!("Change review: refusing edit to {path}: {reason}");
                None
            }
        }
    }

    /// Read `path`'s current whole-file content the same buffer-first way
    /// [`Self::acp_read_text_file`] does, first resolved within the
    /// workspace roots (matching where [`Self::acp_write_text_file`] will
    /// eventually write) — the "current contents"
    /// [`crate::core::review::resolve_fragment`] needs in order to safely
    /// locate a `diff` block's fragment (#1454).
    fn acp_current_file_content(&self, path: &Path) -> Result<String, String> {
        let roots = self.acp_workspace_roots();
        let resolved = crate::core::acp::resolve_path_within_roots(path, &roots)?;
        self.acp_read_text_file(&resolved, None, None)
    }

    /// Human-readable summary of the ACP agent's declared modes and which
    /// one is current, for `:AiMode` with no argument.
    pub(crate) fn acp_mode_status_line(&self) -> String {
        if self.acp_modes.is_empty() {
            return "ACP agent has no modes".to_string();
        }
        let names: Vec<String> = self
            .acp_modes
            .iter()
            .map(|m| {
                if Some(m.id.as_str()) == self.acp_current_mode_id.as_deref() {
                    format!("*{}", m.name)
                } else {
                    m.name.clone()
                }
            })
            .collect();
        format!("Modes: {}", names.join(", "))
    }

    /// Send `session/set_mode` for `target`, matched against the agent's
    /// declared modes by id or name (case-insensitive). The displayed mode
    /// does **not** change optimistically here — it only changes once the
    /// agent's own `current_mode_update` notification lands
    /// (`Self::acp_handle_session_update`), matching #956's "displayed mode
    /// follows `current_mode_update`" acceptance criterion rather than the
    /// request succeeding.
    pub(crate) fn acp_set_mode(&mut self, target: &str) {
        let Some(session_id) = self.acp_session_id.clone() else {
            self.message = "No active ACP session".to_string();
            return;
        };
        let Some(mode) = self
            .acp_modes
            .iter()
            .find(|m| m.id.eq_ignore_ascii_case(target) || m.name.eq_ignore_ascii_case(target))
        else {
            self.message = format!("Unknown ACP mode: {target}");
            return;
        };
        let mode_id = mode.id.clone();
        if let Some(client) = self.acp_client.as_mut() {
            client.set_mode(&session_id, &mode_id);
            self.message = format!("Switching to mode: {mode_id}\u{2026}");
        }
    }

    // ── session/request_permission (#953, ACP-2) ────────────────────────────

    /// Handle a parked `session/request_permission` request: either
    /// auto-answer it from a remembered `allow_always`/`reject_always`
    /// decision, or park it behind the `"acp_permission"` dialog
    /// (`show_dialog`, `Dialog`/`DialogButton` — no new rendering surface;
    /// `body`/`buttons` are already the generic `Vec<String>`/
    /// `Vec<DialogButton>` shape every other dialog uses) for a human to
    /// answer.
    ///
    /// A malformed request (missing `sessionId`/`toolCall`, no options)
    /// still gets exactly one reply — a JSON-RPC error, since there is
    /// nothing a human could meaningfully select — never silence.
    ///
    /// Unlike the `SessionUpdate` handler a few lines above this in
    /// `poll_acp` (which filters on `self.acp_session_id`), this does not
    /// check `req.session_id` before opening the dialog. That's harmless
    /// today — only one `AcpClient`/session is ever live at a time, so
    /// there is no *other* session's request this could ever be — but if a
    /// future slice ever hosts more than one concurrent session, add the
    /// same filter here for symmetry (a permission prompt is far more
    /// consequential to mis-route than a transcript chunk).
    fn acp_handle_permission_request(&mut self, request_id: i64, params: serde_json::Value) {
        let Some(req) = crate::core::acp::parse_request_permission(&params) else {
            if let Some(client) = self.acp_client.as_ref() {
                client.respond_to_client_request(
                    request_id,
                    Err((
                        -32602,
                        "invalid session/request_permission params".to_string(),
                    )),
                );
            }
            return;
        };

        // A prior request_permission that's somehow still parked (the fake
        // fixture and any spec-conformant real agent block for one reply
        // at a time, but never silently strand a reply if that ever
        // isn't true) must get its one reply before this one takes over
        // the dialog.
        self.acp_cancel_pending_permission();

        // Session-scoped remembered decision (#953's `allow_always`/
        // `reject_always`): if the human already decided this tool-call
        // *kind* earlier in this session, answer immediately without
        // reopening the dialog. Falls through to the dialog if this
        // request's own `options` don't offer a matching option kind (an
        // agent is free to omit "always" options on a later ask).
        if let Some(&always_allow) = self.acp_remembered_decisions.get(&req.tool_call.kind) {
            let wanted_prefix = if always_allow { "allow_" } else { "reject_" };
            if let Some(opt) = req
                .options
                .iter()
                .find(|o| o.kind.starts_with(wanted_prefix))
            {
                if let Some(client) = self.acp_client.as_ref() {
                    client.respond_to_client_request(
                        request_id,
                        Ok(crate::core::acp::permission_outcome_selected(
                            &opt.option_id,
                        )),
                    );
                }
                return;
            }
        }

        let title = req.tool_call.title.clone();
        let mut body = vec![format!("Tool kind: {}", req.tool_call.kind)];
        if !req.tool_call.locations.is_empty() {
            body.push(String::new());
            for (path, line) in &req.tool_call.locations {
                body.push(match line {
                    Some(l) => format!("  {path}:{l}"),
                    None => format!("  {path}"),
                });
            }
        }
        // Hotkeys must be unique across this dialog's own buttons — a naive
        // "first letter of the name" pick collides for common ACP option
        // pairs like "Allow Once" / "Always Allow" (both 'a'), and
        // `handle_dialog_key`'s hotkey scan matches the *first* button with
        // a given letter, so a collision silently makes every button after
        // the first one keyboard-unreachable by hotkey (#953 review). Scan
        // each option's own name left-to-right for the first alphabetic
        // character not already claimed by an earlier button in this same
        // dialog; fall back to no hotkey (`'\0'`, still selectable by mouse
        // or arrow-key navigation + Enter) if the name has none left to
        // offer.
        let mut used_hotkeys: std::collections::HashSet<char> = std::collections::HashSet::new();
        let buttons: Vec<DialogButton> = req
            .options
            .iter()
            .map(|opt| {
                let hotkey = opt
                    .name
                    .chars()
                    .map(|c| c.to_ascii_lowercase())
                    .find(|c| c.is_ascii_alphabetic() && !used_hotkeys.contains(c))
                    .unwrap_or('\0');
                if hotkey != '\0' {
                    used_hotkeys.insert(hotkey);
                }
                DialogButton {
                    label: opt.name.clone(),
                    hotkey,
                    action: opt.option_id.clone(),
                }
            })
            .collect();

        // `show_dialog` itself guards against a *stale* `acp_permission`
        // dialog by cancelling it (`acp_cancel_pending_permission`) before
        // opening whatever's requested — including this very one. Set the
        // new pending request only *after* that call, or the guard would
        // immediately cancel the request this method is in the middle of
        // parking.
        self.show_dialog("acp_permission", &title, body, buttons);
        self.acp_pending_permission = Some((request_id, req));
    }

    /// Reply `cancelled` to a parked `session/request_permission` and close
    /// its dialog, if one is open — a no-op otherwise. The one function
    /// every "how does an open permission prompt get answered" path other
    /// than the human's own button choice funnels through (`session/cancel`
    /// via `acp_cancel_turn`, and `show_dialog`'s own guard below for an
    /// unrelated dialog replacing it), so "every path out of the dialog
    /// produces exactly one reply" (#953) is enforced in one place instead
    /// of re-derived at each call site. Does NOT handle the agent-died case
    /// — see `poll_acp`'s `AgentExited` arm, which must not write to a dead
    /// stdin and clears the same state without calling this.
    pub(crate) fn acp_cancel_pending_permission(&mut self) {
        if let Some((request_id, _)) = self.acp_pending_permission.take() {
            if let Some(client) = self.acp_client.as_ref() {
                client.respond_to_client_request(
                    request_id,
                    Ok(crate::core::acp::permission_outcome_cancelled()),
                );
            }
            if self
                .dialog
                .as_ref()
                .is_some_and(|d| d.tag == "acp_permission")
            {
                self.dialog = None;
            }
        }
    }

    /// Abort the in-flight ACP turn (#953: "the user must be able to abort
    /// a running turn from the panel"). Sends `session/cancel`, replies
    /// `cancelled` to any open permission dialog first (never leaves it
    /// parked once the turn it belonged to is being torn down), and clears
    /// the panel's busy state immediately rather than waiting for a
    /// `PromptStopped` that a hung/misbehaving agent might never send.
    ///
    /// A no-op when no ACP session is running — callers don't need to
    /// guard on that themselves.
    pub fn acp_cancel_turn(&mut self) {
        let Some(session_id) = self.acp_session_id.clone() else {
            return;
        };
        self.acp_cancel_pending_permission();
        if let Some(client) = self.acp_client.as_ref() {
            client.cancel(&session_id);
        }
        if self.ai_streaming {
            self.ai_streaming = false;
            self.acp_streaming_turn = None;
            self.ai_messages.push(AiMessage {
                role: "assistant-thought".to_string(),
                content: "[cancelled by user]".to_string(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::engine::Engine;
    use crate::core::lsp::{self, FormattingEdit, WorkspaceEdit};
    use crate::core::{Cursor, Mode};
    use std::path::PathBuf;

    #[test]
    fn poll_acp_is_a_no_op_when_no_client_is_running() {
        let mut engine = Engine::new_for_test();
        assert!(engine.acp_client.is_none());
        assert!(!engine.poll_acp());
    }

    /// An `Engine` holding a freshly-`initialize`d fake ACP agent.
    ///
    /// Full transport-level lifecycle coverage (initialize -> session/new ->
    /// session/prompt -> stopReason: end_turn, and the bidirectional
    /// dispatch/parking acceptance criteria) lives in `src/core/acp.rs`'s own
    /// tests against this same fixture — that's the transport's contract, not
    /// the engine's. What the engine adds on top is exactly one thing:
    /// draining `AgentExited` clears `acp_client` and surfaces a message.
    /// The two tests below cover that from both sides, with the real fixture
    /// rather than by re-deriving the whole lifecycle.
    #[cfg(unix)]
    fn engine_with_fixture_agent(extra_env: &[(&str, &str)]) -> Engine {
        let argv = vec![
            "sh".to_string(),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            )
            .to_string(),
        ];
        let cwd = std::env::temp_dir();
        let mut client = crate::core::acp::AcpClient::spawn_with_env(&argv, &cwd, extra_env)
            .expect("fixture agent should spawn");
        client.initialize();

        let mut engine = Engine::new_for_test();
        engine.acp_client = Some(client);
        engine
    }

    /// #1374 (review round 1): on an account with no zsh startup files — a
    /// freshly provisioned CI runner account is the realistic case, since
    /// any real user who has already installed/configured vimcode and an
    /// ACP agent has used a terminal before and therefore has dotfiles —
    /// interactively spawning zsh auto-launches its own
    /// `zsh-newuser-install` wizard before running anything else. The
    /// wizard reads exactly one raw keystroke to pick a menu option, which
    /// swallows the first character of whatever
    /// `Engine::acp_launch_terminal_login` injects into the shell right
    /// after spawn — every test below that drives a `type: "terminal"`
    /// auth method through to a real login pane (and this file's GTK/TUI
    /// driver twins, `gtk::testing::…ai_panel_terminal_auth_choice_…` and
    /// `tui_main::shell_app::tests::…ai_panel_terminal_auth_choice_…`) hit
    /// exactly this on a bare macOS CI runner.
    ///
    /// The first attempt at this fix (#1374 round 1) worked around it in
    /// *production* code, unconditionally prefixing the injected command
    /// with a bare `q\n` (the wizard's own "quit and do nothing" option) —
    /// rejected on review: it is a real, if narrow, user-visible behaviour
    /// change for every real login on every Unix shell (a stray `q` /
    /// "command not found" line ahead of the real prompt), and outright
    /// breaks the flow for any user with a `q` alias (e.g. `alias
    /// q=exit`, common pager/vim muscle memory). The bare CI account with
    /// no dotfiles is a property of the *test runner*, not of real users —
    /// #1374's own root-cause writeup already frames it as narrow — so per
    /// this repo's rule for fixture problems (as in #1350), the fix
    /// belongs in test setup, not product code.
    ///
    /// Pointing `$ZDOTDIR` at a throwaway directory that already contains
    /// empty `.zshenv`/`.zprofile`/`.zshrc`/`.zlogin` files satisfies
    /// zsh's own "not a brand-new account" check directly — no injected
    /// keystroke or command line involved at all, so there is nothing for
    /// an alias or a wizard-absent prompt to mis-swallow. `$ZDOTDIR` is
    /// read by zsh alone; nothing else in vimcode or its tests consults
    /// it, so — unlike `$HOME` (see `core::paths::TEST_HOME_OVERRIDE`'s
    /// doc comment for the real cross-test corruption a shared `$HOME`
    /// mutation caused, #957 smoke) — overriding it for the rest of the
    /// test process can't disturb any other test's path resolution.
    /// `std::sync::Once` makes the one-time global mutation race-free
    /// against `cargo test`'s parallel threads without needing a
    /// restore-on-drop guard: every caller wants the exact same "some
    /// directory with empty dotfiles" state (unlike e.g.
    /// `VIMCODE_TEST_DATA_HOME`, where distinct tests need distinct
    /// throwaway values and therefore do need a lock), so there is nothing
    /// to race over, and leaving the override in place for the rest of the
    /// process is harmless — it only ever affects a freshly spawned
    /// interactive zsh, and does so by *suppressing* wizard behaviour that
    /// no test anywhere in this suite wants to exercise.
    #[cfg(unix)]
    fn ensure_no_zsh_newuser_wizard() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let dir =
                std::env::temp_dir().join(format!("vimcode_test_zdotdir_{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            for name in [".zshenv", ".zprofile", ".zshrc", ".zlogin"] {
                let _ = std::fs::write(dir.join(name), "");
            }
            std::env::set_var("ZDOTDIR", &dir);
        });
    }

    /// How long the fixture-backed tests below wait before giving up.
    ///
    /// Kept in step with `core::acp::tests::TEST_DEADLINE` (private to that
    /// module, hence the duplicate) and generous for the same reason: these
    /// loops exit the moment their condition holds, so the bound is only
    /// reached on a failing run and a loaded `cargo test` — GTK harness, the
    /// nvim oracles and ~2.7k lib tests all at once — can deschedule a `sh`
    /// fork+exec for far longer than the milliseconds it takes standalone.
    #[cfg(unix)]
    const TEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

    /// Poll `engine` until `done` holds or [`TEST_DEADLINE`] passes.
    #[cfg(unix)]
    fn poll_acp_until(engine: &mut Engine, done: impl Fn(&Engine) -> bool) {
        let start = std::time::Instant::now();
        loop {
            engine.poll_acp();
            if done(engine) || start.elapsed() > TEST_DEADLINE {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn poll_acp_drains_agent_exit_into_the_status_message_and_clears_the_client() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_DIE_AFTER_INIT", "1")]);

        // Deliberately *one* loop, not "first poll sees Initialized, second
        // poll sees AgentExited": `AcpClient::poll` drains whatever the
        // reader thread has queued at that instant, and this fixture answers
        // `initialize` and exits in the same breath, so both events can land
        // in a single drain. A staged version of this test asserted
        // `acp_client.is_some()` between the two polls and failed at random
        // whenever they batched — the flake seen at #984's test stage. The
        // engine contract under test is the end state, and
        // `poll_acp_stays_alive_while_the_agent_is_alive` below covers the
        // "doesn't clear it early" half deterministically, against an agent
        // that is still running rather than against a scheduling race.
        poll_acp_until(&mut engine, |e| e.acp_client.is_none());
        assert!(
            engine.acp_client.is_none(),
            "agent exit should clear the client within {TEST_DEADLINE:?} \
             (message was {:?})",
            engine.message
        );
        assert_eq!(engine.message, "ACP agent exited");
    }

    /// The other half: an agent that is *alive* must not be cleared, and must
    /// not post the exit message, however many times we poll. Uses the same
    /// fixture without `ACP_FAKE_DIE_AFTER_INIT`, so it stays parked on its
    /// read loop (and is killed by `AcpClient`'s `Drop` when the engine goes
    /// out of scope at the end of the test).
    #[cfg(unix)]
    #[test]
    fn poll_acp_stays_alive_while_the_agent_is_alive() {
        let mut engine = engine_with_fixture_agent(&[]);

        // Wait for the `Initialized` event to actually be drained — `poll_acp`
        // reports a redraw for it — so the assertions below can't pass
        // vacuously by running before the agent ever answered.
        let start = std::time::Instant::now();
        let mut redrew = false;
        while !redrew && start.elapsed() < TEST_DEADLINE {
            redrew = engine.poll_acp();
            if !redrew {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        assert!(
            redrew,
            "the fixture's initialize response should have produced at least one drained event"
        );
        // A few more drains for good measure: nothing further is coming, and
        // none of them may decide the agent has gone away.
        for _ in 0..3 {
            engine.poll_acp();
        }
        assert!(
            engine.acp_client.is_some(),
            "a live agent must stay attached after Initialized"
        );
        assert_ne!(
            engine.message, "ACP agent exited",
            "no exit message while the agent is still running"
        );
    }

    // ── ACP-1 (#952): session lifecycle drive + session/update chunk mapping ──

    /// Full round trip: `Engine::ai_send_message` on an already-`initialize`d
    /// agent queues the prompt until the `session/new` handshake completes
    /// (`poll_acp`'s `Initialized`/`SessionCreated` handling), then the
    /// fixture's `agent_thought_chunk` and two `agent_message_chunk`s land as
    /// two *separate* transcript turns — the thought kept visually distinct
    /// from the message per ACP-1's acceptance criteria — with the two
    /// message chunks merged into one streamed turn, not two. Uses
    /// `ACP_FAKE_NO_TOOL_REQUEST` so the turn completes without needing the
    /// fs/* bridge (a later ACP slice, #952's own scope note).
    #[cfg(unix)]
    #[test]
    fn ai_send_message_via_acp_streams_thought_and_message_then_completes() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_NO_TOOL_REQUEST", "1")]);
        // Routing to the ACP transport only checks this is non-empty — the
        // client itself is already spawned above, so `ai_send_message`
        // takes the "reuse existing client" branch, not the "spawn a new
        // one from this command line" branch.
        engine.settings.acp_agent_command = "already-spawned-above".to_string();

        engine.ai_send_message("hello agent".to_string());
        assert!(
            engine.ai_streaming,
            "sending a message must mark the panel busy"
        );
        assert_eq!(
            engine.ai_messages.last().map(|m| m.content.as_str()),
            Some("hello agent"),
            "the user's turn should be recorded immediately, before the handshake completes"
        );

        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(
            !engine.ai_streaming,
            "the turn should reach stopReason: end_turn within {TEST_DEADLINE:?}"
        );

        let roles: Vec<&str> = engine.ai_messages.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(
            roles,
            vec!["user", "assistant-thought", "assistant"],
            "thought and message chunks must land as separate turns, not merged \
             into one another: {:?}",
            engine.ai_messages
        );
        assert_eq!(engine.ai_messages[0].content, "hello agent");
        assert!(
            engine.ai_messages[1].content.contains("pondering"),
            "thought turn content: {:?}",
            engine.ai_messages[1]
        );
        assert_eq!(
            engine.ai_messages[2].content, "Hello world",
            "the two agent_message_chunk notifications must merge into one \
             streamed turn, not create a turn each"
        );
    }

    // ── #1449: attach the current buffer + `@`-mentions ──────────────────

    /// Path the fixture's `ACP_FAKE_CAPTURE_PROMPT_TO` writes each captured
    /// `session/prompt` request line to, one per test so parallel `cargo
    /// test` runs never collide.
    #[cfg(unix)]
    fn capture_file_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "vimcode_test_acp1449_capture_{tag}_{}",
            std::process::id()
        ))
    }

    /// Read back the last captured `session/prompt` request line (the
    /// fixture appends, so the *last* line is the most recent turn) and
    /// return its `params.prompt` content-block array.
    #[cfg(unix)]
    fn captured_prompt_blocks(path: &std::path::Path) -> Vec<serde_json::Value> {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("capture file {} should exist: {e}", path.display()));
        let last_line = content
            .lines()
            .next_back()
            .expect("capture file should have at least one captured line");
        let parsed: serde_json::Value =
            serde_json::from_str(last_line).expect("captured line should be valid JSON");
        parsed["params"]["prompt"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// Acceptance: `ai_attach_current_buffer` on (the default) plus an
    /// active buffer with a real path must add exactly one `resource_link`
    /// content block naming that file, AND show a `⧉`-prefixed chip line
    /// above the user's text in the displayed transcript — the actual wire
    /// content and what the user sees must never disagree about what got
    /// attached (see `Engine::acp_current_buffer_attachment`'s doc).
    ///
    /// RED verified: with `Engine::acp_current_buffer_attachment` stubbed
    /// to always return `None`, this fails on both assertions (no
    /// `resource_link` block on the wire, no `⧉` chip in the transcript).
    #[cfg(unix)]
    #[test]
    fn ai_send_message_via_acp_attaches_current_buffer_as_resource_link() {
        let capture = capture_file_path("attach_on");
        let _ = std::fs::remove_file(&capture);
        let mut engine = engine_with_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_CAPTURE_PROMPT_TO", capture.to_str().unwrap()),
        ]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        assert!(
            engine.settings.ai_attach_current_buffer,
            "attach-current-buffer must default to on"
        );

        let workspace = std::env::temp_dir();
        engine.workspace_root = Some(workspace.clone());
        let file_path = workspace.join(format!(
            "vimcode_test_acp1449_buf_{}.rs",
            std::process::id()
        ));
        std::fs::write(&file_path, "fn main() {}\n").expect("write test buffer file");
        engine.active_buffer_state_mut().file_path = Some(file_path.clone());

        engine.ai_send_message("hello agent".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(!engine.ai_streaming, "turn should complete within deadline");

        let blocks = captured_prompt_blocks(&capture);
        let expected_uri =
            crate::core::lsp::path_to_uri(&file_path.canonicalize().expect("file exists"));
        assert!(
            blocks
                .iter()
                .any(|b| b["type"] == "resource_link" && b["uri"] == expected_uri),
            "expected a resource_link for the attached buffer at {expected_uri}; \
             captured blocks: {blocks:?}"
        );
        assert!(
            engine.ai_messages[0].content.contains('\u{29c9}'),
            "displayed user turn should show the attachment chip: {:?}",
            engine.ai_messages[0]
        );

        let _ = std::fs::remove_file(&file_path);
        let _ = std::fs::remove_file(&capture);
    }

    /// Acceptance: turning `ai_attach_current_buffer` off must send no
    /// `resource_link` at all and show no chip, even with an active buffer
    /// that has a real path.
    #[cfg(unix)]
    #[test]
    fn ai_send_message_via_acp_skips_attachment_when_setting_is_off() {
        let capture = capture_file_path("attach_off");
        let _ = std::fs::remove_file(&capture);
        let mut engine = engine_with_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_CAPTURE_PROMPT_TO", capture.to_str().unwrap()),
        ]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.settings.ai_attach_current_buffer = false;

        let workspace = std::env::temp_dir();
        engine.workspace_root = Some(workspace.clone());
        let file_path = workspace.join(format!(
            "vimcode_test_acp1449_buf_off_{}.rs",
            std::process::id()
        ));
        std::fs::write(&file_path, "fn main() {}\n").expect("write test buffer file");
        engine.active_buffer_state_mut().file_path = Some(file_path.clone());

        engine.ai_send_message("hello agent".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(!engine.ai_streaming, "turn should complete within deadline");

        let blocks = captured_prompt_blocks(&capture);
        assert!(
            !blocks.iter().any(|b| b["type"] == "resource_link"),
            "setting off must send no resource_link at all: {blocks:?}"
        );
        assert!(
            !engine.ai_messages[0].content.contains('\u{29c9}'),
            "setting off must show no attachment chip: {:?}",
            engine.ai_messages[0]
        );

        let _ = std::fs::remove_file(&file_path);
        let _ = std::fs::remove_file(&capture);
    }

    /// Acceptance: an unnamed/scratch buffer (the default buffer every
    /// fresh `Engine` starts with) has nothing on disk to link — no
    /// `resource_link`, no chip — even with the setting on.
    #[cfg(unix)]
    #[test]
    fn ai_send_message_via_acp_skips_attachment_for_scratch_buffer() {
        let capture = capture_file_path("scratch");
        let _ = std::fs::remove_file(&capture);
        let mut engine = engine_with_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_CAPTURE_PROMPT_TO", capture.to_str().unwrap()),
        ]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        assert!(
            engine.active_buffer_state().file_path.is_none(),
            "a fresh engine's default buffer must be an unnamed scratch buffer"
        );

        engine.ai_send_message("hello agent".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(!engine.ai_streaming, "turn should complete within deadline");

        let blocks = captured_prompt_blocks(&capture);
        assert!(
            !blocks.iter().any(|b| b["type"] == "resource_link"),
            "a scratch buffer has nothing to attach: {blocks:?}"
        );
        assert!(
            !engine.ai_messages[0].content.contains('\u{29c9}'),
            "a scratch buffer must show no attachment chip: {:?}",
            engine.ai_messages[0]
        );

        let _ = std::fs::remove_file(&capture);
    }

    /// Acceptance: a literal `@path` mention still present in the submitted
    /// text resolves to its own `resource_link` block, and the text block
    /// keeps the mention exactly as typed (nothing is stripped out of the
    /// message the user actually sees on either end).
    ///
    /// RED verified: with `Engine::acp_prompt_content_blocks`'s mention
    /// loop deleted, the wire content has only the text block and no
    /// `resource_link` for the mentioned file.
    #[cfg(unix)]
    #[test]
    fn ai_send_message_via_acp_mention_resolves_to_resource_link_and_keeps_literal_text() {
        let capture = capture_file_path("mention");
        let _ = std::fs::remove_file(&capture);
        let mut engine = engine_with_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_CAPTURE_PROMPT_TO", capture.to_str().unwrap()),
        ]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        // Isolate the mention's own block from the current-buffer one.
        engine.settings.ai_attach_current_buffer = false;

        let workspace = std::env::temp_dir();
        engine.workspace_root = Some(workspace.clone());
        let mentioned = workspace.join(format!(
            "vimcode_test_acp1449_mention_{}.rs",
            std::process::id()
        ));
        std::fs::write(&mentioned, "// mentioned\n").expect("write mentioned file");
        let mention_name = mentioned.file_name().unwrap().to_string_lossy().to_string();

        let text = format!("please check @{mention_name} for bugs");
        engine.ai_send_message(text.clone());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(!engine.ai_streaming, "turn should complete within deadline");

        let blocks = captured_prompt_blocks(&capture);
        let expected_uri =
            crate::core::lsp::path_to_uri(&mentioned.canonicalize().expect("file exists"));
        assert!(
            blocks
                .iter()
                .any(|b| b["type"] == "resource_link" && b["uri"] == expected_uri),
            "expected a resource_link for the mentioned file at {expected_uri}; \
             captured blocks: {blocks:?}"
        );
        assert!(
            blocks
                .iter()
                .any(|b| b["type"] == "text" && b["text"] == text),
            "the literal @mention text must stay in the text block unchanged: {blocks:?}"
        );

        let _ = std::fs::remove_file(&mentioned);
        let _ = std::fs::remove_file(&capture);
    }

    /// #1449 acceptance: `@`-mention completions list open buffers before
    /// workspace files sharing the same prefix, and both sources are
    /// searched (not just one). No live ACP agent needed — this is pure
    /// `Engine::ai_mention_completions` state, the same "no subprocess
    /// needed" tier as the pure parsing tests in `core::acp`.
    #[test]
    fn ai_mention_completions_lists_open_buffer_before_workspace_only_file() {
        let mut engine = Engine::new_for_test();
        let workspace = std::env::temp_dir().join(format!(
            "vimcode_test_acp1449_mention_ws_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&workspace).expect("create test workspace dir");
        engine.cwd = workspace.clone();
        engine.workspace_root = Some(workspace.clone());

        // An open buffer with a path under the workspace...
        std::fs::write(workspace.join("open_buf.rs"), "").expect("write open buffer file");
        engine.active_buffer_state_mut().file_path = Some(workspace.join("open_buf.rs"));
        // ...plus a file on disk sharing the same prefix that is NOT open
        // in any buffer.
        std::fs::write(workspace.join("open_buf_extra.rs"), "").expect("write extra file");

        engine
            .ai_chat
            .borrow_mut()
            .input_insert_str("look at @open_buf");
        let menu = engine
            .ai_mention_completions()
            .expect("typing @ with a matching prefix should show mention completions");
        assert_eq!(
            menu.candidates[0], "@open_buf.rs",
            "the open buffer must be listed before the on-disk-only file \
             sharing the same prefix: {:?}",
            menu.candidates
        );
        assert!(
            menu.candidates.contains(&"@open_buf_extra.rs".to_string()),
            "the workspace-only file must still be offered: {:?}",
            menu.candidates
        );

        let _ = std::fs::remove_dir_all(&workspace);
    }

    // ── #1450: Visual selection / `:{range}AI` range attachment ──────────

    /// Write `contents` to a fresh temp file and point the active buffer's
    /// `file_path` at it, returning the path for cleanup/comparison. Shared
    /// setup for every #1450 test below — none of them care about anything
    /// but "a real file the active buffer is backed by".
    fn setup_range_attachment_buffer(engine: &mut Engine, tag: &str, contents: &str) -> PathBuf {
        let workspace = std::env::temp_dir();
        engine.workspace_root = Some(workspace.clone());
        let file_path = workspace.join(format!(
            "vimcode_test_acp1450_{tag}_{}.rs",
            std::process::id()
        ));
        std::fs::write(&file_path, contents).expect("write test buffer file");
        engine.active_buffer_state_mut().file_path = Some(file_path.clone());
        // Load the same content into the buffer itself — `:{range}AI`/the
        // Visual mapping read the *live* buffer, not the file on disk, so a
        // test that only wrote the file (leaving the buffer's default empty
        // rope) would pass vacuously with an empty attachment text.
        let len = engine.buffer().len_chars();
        engine.delete_with_undo(0, len);
        engine.insert_with_undo(0, contents);
        file_path
    }

    /// Acceptance (point 3, `embeddedContext: true`): `:{range}AI` sends a
    /// single `resource` block whose `resource.text` is the exact buffer
    /// text for that range — including an edit made after the file was
    /// written to disk (unsaved edits included, per the issue) — and whose
    /// `resource.uri` carries the `#L<start>-L<end>` line-range fragment.
    ///
    /// RED verified: with `AcpRangeAttachment::content_blocks` stubbed to
    /// always return the `resource_link` fallback shape, this fails (no
    /// `resource` block on the wire at all).
    #[cfg(unix)]
    #[test]
    fn ranged_ai_command_with_embedded_context_sends_exact_unsaved_text_and_range() {
        let capture = capture_file_path("range_embedded");
        let _ = std::fs::remove_file(&capture);
        let mut engine = engine_with_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_CAPTURE_PROMPT_TO", capture.to_str().unwrap()),
            ("ACP_FAKE_EMBEDDED_CONTEXT", "1"),
        ]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.settings.ai_attach_current_buffer = false;
        let file_path = setup_range_attachment_buffer(
            &mut engine,
            "embedded",
            "fn one() {}\nfn two() {}\nfn three() {}\n",
        );
        // An unsaved edit — never written back to `file_path` — must still
        // show up verbatim in the attached text.
        let line1_start = engine.buffer().line_to_char(1);
        engine.insert_with_undo(line1_start, "// unsaved edit\n");

        // Wait for `Initialized` (and its `promptCapabilities`) to drain
        // before staging the attachment/sending, so the capability is
        // already known the moment `acp_prompt_content_blocks` runs.
        poll_acp_until(&mut engine, |e| e.acp_prompt_capabilities.embedded_context);
        assert!(
            engine.acp_prompt_capabilities.embedded_context,
            "fixture should have advertised embeddedContext: true"
        );

        // Lines 1-2 (0-based), i.e. the unsaved comment plus "fn two() {}".
        engine.ai_attach_range(1, 2, "explain these lines");
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(!engine.ai_streaming, "turn should complete within deadline");

        let blocks = captured_prompt_blocks(&capture);
        let resource = blocks
            .iter()
            .find(|b| b["type"] == "resource")
            .unwrap_or_else(|| panic!("expected a resource block: {blocks:?}"));
        assert_eq!(
            resource["resource"]["text"], "// unsaved edit\nfn two() {}\n",
            "attached text must be the live buffer content, unsaved edit included"
        );
        let expected_uri = format!(
            "{}#L2-L3",
            crate::core::lsp::path_to_uri(&file_path.canonicalize().expect("file exists"))
        );
        assert_eq!(
            resource["resource"]["uri"], expected_uri,
            "the URI must carry the 1-based #L2-L3 range for 0-based lines 1-2"
        );
        assert_eq!(resource["resource"]["mimeType"], "text/x-rust");
        assert!(
            !blocks.iter().any(|b| b["type"] == "resource_link"),
            "the range attachment must not also send the whole-file fallback \
             shape when embeddedContext is true (ai_attach_current_buffer is \
             off in this test): {blocks:?}"
        );

        let _ = std::fs::remove_file(&file_path);
        let _ = std::fs::remove_file(&capture);
    }

    /// Acceptance (point 3, no `embeddedContext`): without the capability,
    /// `:{range}AI` falls back to a `resource_link` for the file plus a
    /// `text` block naming the path/range with a fenced copy of the exact
    /// selection — never a `resource` block.
    ///
    /// RED verified: with `AcpPromptCapabilities::embedded_context` ignored
    /// and `content_blocks` always taking the `resource` branch, this fails
    /// (a `resource` block appears where a `resource_link` + fenced text
    /// was expected).
    #[cfg(unix)]
    #[test]
    fn ranged_ai_command_without_embedded_context_falls_back_to_resource_link_and_fenced_text() {
        let capture = capture_file_path("range_fallback");
        let _ = std::fs::remove_file(&capture);
        let mut engine = engine_with_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_CAPTURE_PROMPT_TO", capture.to_str().unwrap()),
        ]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.settings.ai_attach_current_buffer = false;
        let file_path = setup_range_attachment_buffer(
            &mut engine,
            "fallback",
            "fn one() {}\nfn two() {}\nfn three() {}\n",
        );
        poll_acp_until(&mut engine, |e| {
            e.acp_session_id.is_some() || e.acp_client.is_none()
        });
        assert!(
            !engine.acp_prompt_capabilities.embedded_context,
            "fixture must not advertise embeddedContext by default"
        );

        engine.ai_attach_range(0, 1, "explain these lines");
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(!engine.ai_streaming, "turn should complete within deadline");

        let blocks = captured_prompt_blocks(&capture);
        assert!(
            !blocks.iter().any(|b| b["type"] == "resource"),
            "must not send an embedded `resource` block without the capability: {blocks:?}"
        );
        let expected_uri =
            crate::core::lsp::path_to_uri(&file_path.canonicalize().expect("file exists"));
        assert!(
            blocks
                .iter()
                .any(|b| b["type"] == "resource_link" && b["uri"] == expected_uri),
            "expected a resource_link fallback for the file: {blocks:?}"
        );
        let fenced = blocks
            .iter()
            .find(|b| b["type"] == "text" && b["text"] != "explain these lines")
            .unwrap_or_else(|| panic!("expected a fenced-text fallback block: {blocks:?}"));
        let fenced_text = fenced["text"].as_str().unwrap_or_default();
        assert!(
            fenced_text.contains("fn one() {}\nfn two() {}\n"),
            "fenced fallback must contain the exact selected lines: {fenced_text:?}"
        );
        assert!(
            fenced_text.contains("lines 1-2"),
            "fenced fallback must name the 1-based line range: {fenced_text:?}"
        );

        let _ = std::fs::remove_file(&file_path);
        let _ = std::fs::remove_file(&capture);
    }

    /// Acceptance (point 1): `:'<,'>AI` with no message stages the
    /// attachment and focuses the panel without sending — the message can
    /// still be typed and submitted afterward.
    #[test]
    fn ai_attach_range_with_no_message_stages_attachment_and_focuses_without_sending() {
        let mut engine = Engine::new_for_test();
        let file_path = setup_range_attachment_buffer(&mut engine, "stage", "one\ntwo\nthree\n");

        assert!(!engine.ai_has_focus);
        engine.ai_attach_range(0, 1, "");

        assert!(
            engine.ai_has_focus,
            "an empty message must still focus the panel"
        );
        assert!(engine.ai_messages.is_empty(), "nothing should be sent yet");
        let attachment = engine
            .acp_pending_attachment
            .as_ref()
            .expect("attachment should be staged");
        assert_eq!(attachment.start_line, 0);
        assert_eq!(attachment.end_line, 1);
        assert_eq!(attachment.text, "one\ntwo\n");

        let _ = std::fs::remove_file(&file_path);
    }

    /// Acceptance (point 5): the Visual mapping's characterwise selection
    /// keeps the exact selected text, not whole lines — even though the
    /// selection spans only part of each of two lines.
    #[test]
    fn visual_mapping_characterwise_selection_attaches_exact_text_not_whole_lines() {
        let mut engine = Engine::new_for_test();
        let file_path =
            setup_range_attachment_buffer(&mut engine, "charwise", "abcdefgh\nijklmnop\n");

        // Select from col 3 on line 0 ('d') through col 2 on line 1 ('k'),
        // inclusive — a characterwise span crossing a line boundary.
        engine.mode = Mode::Visual;
        engine.visual_anchor = Some(Cursor { line: 0, col: 3 });
        engine.view_mut().cursor = Cursor { line: 1, col: 2 };

        engine.acp_attach_visual_selection_and_focus();

        assert_eq!(engine.mode, Mode::Normal, "should exit Visual mode");
        assert!(engine.ai_has_focus);
        let attachment = engine
            .acp_pending_attachment
            .as_ref()
            .expect("attachment should be staged");
        assert_eq!(
            attachment.text, "defgh\nijk",
            "must be the exact characterwise span, not whole lines 0-1"
        );
        assert_eq!(attachment.start_line, 0);
        assert_eq!(attachment.end_line, 1);

        let _ = std::fs::remove_file(&file_path);
    }

    /// Acceptance (point 5): a linewise (`V`) Visual selection attaches
    /// whole lines, same as the ex-range form — the "exact text" carve-out
    /// is specific to characterwise/blockwise.
    #[test]
    fn visual_mapping_linewise_selection_attaches_whole_lines() {
        let mut engine = Engine::new_for_test();
        let file_path =
            setup_range_attachment_buffer(&mut engine, "linewise", "abcdefgh\nijklmnop\n");

        engine.mode = Mode::VisualLine;
        engine.visual_anchor = Some(Cursor { line: 0, col: 3 });
        engine.view_mut().cursor = Cursor { line: 0, col: 3 };

        engine.acp_attach_visual_selection_and_focus();

        let attachment = engine
            .acp_pending_attachment
            .as_ref()
            .expect("attachment should be staged");
        assert_eq!(
            attachment.text, "abcdefgh\n",
            "linewise must attach the whole line, not just the column span"
        );

        let _ = std::fs::remove_file(&file_path);
    }

    /// Acceptance: outside Visual mode, the `<leader>ai` mapping just
    /// focuses the panel — no attachment, matching the palette's plain
    /// `chat_open` fallback.
    #[test]
    fn visual_mapping_outside_visual_mode_only_focuses_panel() {
        let mut engine = Engine::new_for_test();
        assert_eq!(engine.mode, Mode::Normal);
        engine.acp_attach_visual_selection_and_focus();
        assert!(engine.ai_has_focus);
        assert!(engine.acp_pending_attachment.is_none());
    }

    /// Acceptance (point 4): Ctrl+R while the panel has focus removes a
    /// staged attachment without sending anything.
    #[test]
    fn ctrl_r_removes_a_staged_attachment_without_sending() {
        let mut engine = Engine::new_for_test();
        let file_path = setup_range_attachment_buffer(&mut engine, "remove", "one\ntwo\n");
        engine.ai_attach_range(0, 1, "");
        assert!(engine.acp_pending_attachment.is_some());

        let kept_focus = engine.dispatch_ai_chat_event(quadraui::ChatControllerEvent::KeyPressed {
            key: "Char('r')".to_string(),
            modifiers: quadraui::Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
                cmd: false,
            },
        });

        assert!(kept_focus, "Ctrl+R must not drop panel focus");
        assert!(
            engine.acp_pending_attachment.is_none(),
            "attachment must be removed"
        );
        assert!(engine.ai_messages.is_empty(), "nothing should be sent");

        let _ = std::fs::remove_file(&file_path);
    }

    /// #1464: with no range attachment staged, Ctrl+R falls through to
    /// popping the most-recently manually-attached file/image instead —
    /// same "one key, most-recent-first" removal `dispatch_ai_chat_event`'s
    /// doc promises, just the other one of the two attachment kinds it
    /// checks. (`setup_attach_workspace` is defined further down in this
    /// module alongside the rest of the `:AiAttach` tests.)
    ///
    /// RED verified: with the `else if let Some(removed) =
    /// self.acp_manual_attachments.pop()` fallthrough branch stubbed out
    /// (leaving only the range-attachment check), this fails —
    /// `acp_manual_attachments` still holds both attachments after Ctrl+R.
    #[cfg(unix)]
    #[test]
    fn ctrl_r_falls_through_to_popping_the_last_manual_attachment_when_none_is_staged() {
        let mut engine = Engine::new_for_test();
        let (name, file_path) = setup_attach_workspace(&mut engine, "ctrlr", "notes.txt", b"hi");
        engine.acp_attach_file(&name);
        engine.acp_prompt_capabilities.image = true;
        let bytes = vec![0x89, 0x50, 0x4e, 0x47];
        let workspace = file_path.parent().unwrap().to_path_buf();
        std::fs::write(workspace.join("shot.png"), &bytes).expect("write second attach target");
        engine.acp_attach_file("shot.png");
        assert_eq!(
            engine.acp_manual_attachments.len(),
            2,
            "{:?}",
            engine.message
        );
        assert!(engine.acp_pending_attachment.is_none());

        let kept_focus = engine.dispatch_ai_chat_event(quadraui::ChatControllerEvent::KeyPressed {
            key: "Char('r')".to_string(),
            modifiers: quadraui::Modifiers {
                ctrl: true,
                shift: false,
                alt: false,
                cmd: false,
            },
        });

        assert!(kept_focus, "Ctrl+R must not drop panel focus");
        assert_eq!(
            engine.acp_manual_attachments.len(),
            1,
            "only the most-recently attached (the image) should be popped"
        );
        assert_eq!(
            engine.acp_manual_attachments[0].chip(),
            "\u{1f4ce} notes.txt"
        );
        assert!(
            engine.message.contains("shot.png"),
            "removal message should name what was removed: {}",
            engine.message
        );
        assert!(engine.ai_messages.is_empty(), "nothing should be sent");

        let _ = std::fs::remove_dir_all(&workspace);
    }

    /// Acceptance: `ai_clear` drops a staged attachment too — composed but
    /// unsent content, same as everything else it resets.
    #[test]
    fn ai_clear_drops_a_staged_attachment() {
        let mut engine = Engine::new_for_test();
        let file_path = setup_range_attachment_buffer(&mut engine, "clear", "one\ntwo\n");
        engine.ai_attach_range(0, 1, "");
        assert!(engine.acp_pending_attachment.is_some());

        engine.ai_clear();

        assert!(engine.acp_pending_attachment.is_none());
        let _ = std::fs::remove_file(&file_path);
    }

    /// The `⧉`-chip shown while an attachment is staged (point 4) — plain
    /// data-shape coverage; the driver-tier tests
    /// (`tui_main::shell_app`/`gtk::testing`) cover it actually being
    /// painted in the panel header.
    #[test]
    fn range_attachment_chip_format() {
        let workspace = std::path::PathBuf::from("/ws");
        let single = crate::core::acp::AcpRangeAttachment {
            path: std::path::PathBuf::from("/ws/src/main.rs"),
            start_line: 9,
            end_line: 9,
            text: "let x = 1;\n".to_string(),
        };
        assert_eq!(single.chip(&workspace), "\u{29c9} src/main.rs:10");

        let range = crate::core::acp::AcpRangeAttachment {
            path: std::path::PathBuf::from("/ws/src/main.rs"),
            start_line: 9,
            end_line: 23,
            text: String::new(),
        };
        assert_eq!(range.chip(&workspace), "\u{29c9} src/main.rs:10-24");
    }

    /// Review regression (#952): a `RequestFailed` for `session/new` — a
    /// non-fatal JSON-RPC error during the handshake, agent stays alive —
    /// must clear `ai_streaming`/`acp_streaming_turn`/`acp_pending_prompt`
    /// just like a failed `session/prompt` does, not just log the warning.
    /// Before the fix, `poll_acp`'s `RequestFailed` arm only reset the busy
    /// state `if method == "session/prompt"`, so this exact sequence left
    /// `ai_streaming` stuck `true` forever and `ai_send_message` would
    /// silently no-op on every subsequent call (`ext_panel.rs`'s early
    /// return on `self.ai_streaming`) — a permanently wedged panel with no
    /// crash and no further transcript growth.
    ///
    /// RED verified: reverting the `RequestFailed` arm to only reset state
    /// `if method == "session/prompt"` makes this test fail — `ai_streaming`
    /// stays `true` and a follow-up `ai_send_message` is silently dropped.
    #[cfg(unix)]
    #[test]
    fn request_failed_during_session_new_clears_busy_state_not_just_session_prompt() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_SESSION_NEW_ERROR", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();

        engine.ai_send_message("hello agent".to_string());
        assert!(
            engine.ai_streaming,
            "sending a message must mark the panel busy immediately"
        );

        // Wait for the transcript to grow past just the user's turn: the
        // warning `poll_acp` pushes on `RequestFailed` is the signal the
        // error was actually drained (not just that the busy flag flipped
        // some other way).
        poll_acp_until(&mut engine, |e| e.ai_messages.len() > 1);
        assert_eq!(
            engine.ai_messages.len(),
            2,
            "the session/new error should land as one warning turn: {:?}",
            engine.ai_messages
        );
        assert!(
            engine.ai_messages[1].content.contains("session/new failed"),
            "warning should name the failed method: {:?}",
            engine.ai_messages[1]
        );

        assert!(
            !engine.ai_streaming,
            "a non-fatal error response to session/new must clear the busy \
             state, not just a failed session/prompt — otherwise the panel \
             is wedged and silently drops every further message"
        );
        assert!(
            engine.acp_streaming_turn.is_none(),
            "no turn was ever streamed, so this must stay None"
        );
        assert!(
            engine.acp_pending_prompt.is_none(),
            "the queued prompt from the failed handshake must not survive \
             to be replayed against a later, unrelated session"
        );

        // And the panel must actually be usable again, not just internally
        // "not streaming": a second send should reach the transport instead
        // of being silently swallowed by ai_send_message's busy-check
        // (`if text.is_empty() || self.ai_streaming { return; }` in
        // `ext_panel.rs`). Checking `ai_streaming` alone would pass
        // vacuously even with the bug reinstated — it was already `true` —
        // so assert the message was actually recorded.
        engine.ai_send_message("still there?".to_string());
        assert_eq!(
            engine.ai_messages.len(),
            3,
            "a second message after the failed handshake must actually be \
             recorded, not silently dropped by a still-stuck busy flag: {:?}",
            engine.ai_messages
        );
        assert_eq!(engine.ai_messages[2].content, "still there?");
        assert!(
            engine.ai_streaming,
            "the panel must accept a new message after the failed handshake \
             cleared the busy state"
        );
    }

    // ── #1464: manual file/image attachments ──────────────────────────────

    /// Write `contents` (bytes, not necessarily valid UTF-8/a real image —
    /// the fixture agent and `Engine::acp_attach_file` never decode it,
    /// only size-check and base64-encode it) to a fresh temp file under a
    /// fresh temp workspace, pointing `engine.cwd`/`workspace_root` at that
    /// workspace. Returns the file's bare name (what a test passes to
    /// `:AiAttach`) and its full path (for cleanup).
    #[cfg(unix)]
    fn setup_attach_workspace(
        engine: &mut Engine,
        tag: &str,
        name: &str,
        contents: &[u8],
    ) -> (String, PathBuf) {
        let workspace =
            std::env::temp_dir().join(format!("vimcode_test_acp1464_{tag}_{}", std::process::id()));
        std::fs::create_dir_all(&workspace).expect("create test workspace dir");
        let file_path = workspace.join(name);
        std::fs::write(&file_path, contents).expect("write attach target");
        engine.cwd = workspace.clone();
        engine.workspace_root = Some(workspace);
        (name.to_string(), file_path)
    }

    #[cfg(unix)]
    #[test]
    fn ai_attach_file_stages_a_plain_file_as_a_resource_link() {
        let mut engine = Engine::new_for_test();
        let (name, file_path) = setup_attach_workspace(&mut engine, "plain", "notes.txt", b"hi");

        engine.acp_attach_file(&name);

        assert_eq!(
            engine.acp_manual_attachments.len(),
            1,
            "{:?}",
            engine.message
        );
        let block = engine.acp_manual_attachments[0].content_block();
        assert_eq!(block["type"], "resource_link");
        assert_eq!(block["name"], "notes.txt");
        assert!(
            engine.message.contains("Attached"),
            "message: {}",
            engine.message
        );

        let _ = std::fs::remove_dir_all(file_path.parent().unwrap());
    }

    /// RED verified: with `Engine::acp_attach_file`'s image-capability check
    /// removed, this fails — an image attaches (and would later be sent)
    /// even though the agent never declared `promptCapabilities.image`.
    #[cfg(unix)]
    #[test]
    fn ai_attach_file_refuses_an_image_when_the_agent_lacks_the_capability() {
        let mut engine = Engine::new_for_test();
        assert!(!engine.acp_prompt_capabilities.image);
        let (name, file_path) =
            setup_attach_workspace(&mut engine, "noimg", "shot.png", b"not-really-a-png");

        engine.acp_attach_file(&name);

        assert!(
            engine.acp_manual_attachments.is_empty(),
            "refused image must not be staged: {:?}",
            engine.acp_manual_attachments
        );
        assert!(
            engine.message.contains("doesn't support image attachments"),
            "message: {}",
            engine.message
        );

        let _ = std::fs::remove_dir_all(file_path.parent().unwrap());
    }

    /// RED verified: with `content_block`'s `Image` arm stubbed to the
    /// `File` arm's `resource_link` shape, the `block["type"] == "image"`
    /// assertion below fails.
    #[cfg(unix)]
    #[test]
    fn ai_attach_file_attaches_an_image_with_base64_data_when_the_capability_is_present() {
        let mut engine = Engine::new_for_test();
        engine.acp_prompt_capabilities.image = true;
        let bytes = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a];
        let (name, file_path) = setup_attach_workspace(&mut engine, "img", "shot.png", &bytes);

        engine.acp_attach_file(&name);

        assert_eq!(
            engine.acp_manual_attachments.len(),
            1,
            "{:?}",
            engine.message
        );
        let block = engine.acp_manual_attachments[0].content_block();
        assert_eq!(block["type"], "image");
        assert_eq!(block["mimeType"], "image/png");
        use base64::Engine as _;
        assert_eq!(
            block["data"],
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );

        let _ = std::fs::remove_dir_all(file_path.parent().unwrap());
    }

    /// RED verified: with `Engine::acp_attach_file`'s size check removed,
    /// this fails — a 5&nbsp;MiB+1 image attaches instead of being refused.
    #[cfg(unix)]
    #[test]
    fn ai_attach_file_rejects_an_oversize_image() {
        let mut engine = Engine::new_for_test();
        engine.acp_prompt_capabilities.image = true;
        let oversize = vec![0u8; (crate::core::acp::ACP_MAX_IMAGE_ATTACHMENT_BYTES + 1) as usize];
        let (name, file_path) = setup_attach_workspace(&mut engine, "big", "huge.png", &oversize);

        engine.acp_attach_file(&name);

        assert!(
            engine.acp_manual_attachments.is_empty(),
            "oversize image must not be staged: {:?}",
            engine.acp_manual_attachments
        );
        assert!(
            engine.message.contains("larger than the 5.0 MB limit"),
            "message: {}",
            engine.message
        );

        let _ = std::fs::remove_dir_all(file_path.parent().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn ai_attach_file_refuses_a_path_outside_the_workspace() {
        let mut engine = Engine::new_for_test();
        let workspace = std::env::temp_dir().join(format!(
            "vimcode_test_acp1464_outside_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&workspace).expect("create test workspace dir");
        engine.cwd = workspace.clone();
        engine.workspace_root = Some(workspace.clone());

        engine.acp_attach_file("../../etc/passwd");

        assert!(engine.acp_manual_attachments.is_empty());
        assert!(
            engine.message.contains("outside the workspace"),
            "message: {}",
            engine.message
        );

        let _ = std::fs::remove_dir_all(&workspace);
    }

    /// The issue's own acceptance line: "Assert an image block is sent with
    /// the right mime and base64 when the capability is present" — a real
    /// round trip through the fixture agent (`$ACP_FAKE_IMAGE_CAPABILITY`),
    /// not just `Engine::acp_attach_file`'s local staging the tests above
    /// cover, so this also proves `Engine::acp_prompt_content_blocks`
    /// actually drains `acp_manual_attachments` onto the wire.
    ///
    /// RED verified: with `Engine::acp_prompt_content_blocks`'s manual-
    /// attachment drain loop removed, the captured prompt never contains an
    /// `image`-typed block at all.
    #[cfg(unix)]
    #[test]
    fn ranged_ai_attach_sends_an_image_block_with_base64_data_over_the_wire() {
        let capture = capture_file_path("attach_image");
        let _ = std::fs::remove_file(&capture);
        let mut engine = engine_with_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_IMAGE_CAPABILITY", "1"),
            ("ACP_FAKE_CAPTURE_PROMPT_TO", capture.to_str().unwrap()),
        ]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.settings.ai_attach_current_buffer = false;
        poll_acp_until(&mut engine, |e| e.acp_prompt_capabilities.image);
        assert!(
            engine.acp_prompt_capabilities.image,
            "fixture should have advertised promptCapabilities.image: true"
        );

        let bytes = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a];
        let (name, file_path) = setup_attach_workspace(&mut engine, "wire", "shot.png", &bytes);
        engine.acp_attach_file(&name);
        assert_eq!(
            engine.acp_manual_attachments.len(),
            1,
            "{:?}",
            engine.message
        );

        engine.ai_send_message("look at this".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(!engine.ai_streaming, "turn should complete within deadline");

        let blocks = captured_prompt_blocks(&capture);
        let image_block = blocks
            .iter()
            .find(|b| b["type"] == "image")
            .unwrap_or_else(|| panic!("expected an image block: {blocks:?}"));
        assert_eq!(image_block["mimeType"], "image/png");
        use base64::Engine as _;
        assert_eq!(
            image_block["data"],
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        assert!(
            engine.acp_manual_attachments.is_empty(),
            "the attachment must be consumed (drained), not left staged \
             after it was sent"
        );

        let _ = std::fs::remove_dir_all(file_path.parent().unwrap());
        let _ = std::fs::remove_file(&capture);
    }

    // ── #1464: clipboard image paste (`:AiPasteImage`) ──────────────────────
    //
    // Mirrors the `:AiAttach` engine-level tests above, mocking
    // `engine.clipboard_read_image` the same way `engine.clipboard_read` is
    // mocked throughout this crate (see the dozen+ `clipboard_read = Some(
    // Box::new(...))` call sites), rather than driving a real clipboard.

    /// A flat 2x2 opaque-red RGBA8 image — small enough that its PNG
    /// encoding is always well under the size limit, for the "capability
    /// present, paste succeeds" tests.
    fn small_rgba_image() -> quadraui::RgbaImage {
        quadraui::RgbaImage {
            width: 2,
            height: 2,
            pixels: vec![255, 0, 0, 255].repeat(4),
        }
    }

    /// A large, incompressible RGBA8 image whose PNG encoding exceeds
    /// [`crate::core::acp::ACP_MAX_IMAGE_ATTACHMENT_BYTES`] — unlike an
    /// all-zero buffer, deflate can't crush pseudo-random noise down below
    /// the limit, so this actually exercises the *encoded*-size check the
    /// doc on `Engine::acp_attach_clipboard_image` calls out.
    fn oversize_rgba_image() -> quadraui::RgbaImage {
        let (width, height) = (2000u32, 1200u32);
        let len = (width as usize) * (height as usize) * 4;
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let pixels = (0..len)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                (state >> 33) as u8
            })
            .collect();
        quadraui::RgbaImage {
            width,
            height,
            pixels,
        }
    }

    /// RED verified: with the `self.clipboard_read_image.as_ref()` early
    /// return removed (falling through to a bare `Ok`/default image), this
    /// fails — `:AiPasteImage` on a platform/backend that never wired the
    /// callback (TUI, today) would silently invent a paste instead of
    /// refusing.
    #[test]
    fn paste_clipboard_image_refuses_when_no_callback_is_wired() {
        let mut engine = Engine::new_for_test();
        engine.acp_prompt_capabilities.image = true;
        assert!(engine.clipboard_read_image.is_none());

        engine.acp_attach_clipboard_image();

        assert!(engine.acp_manual_attachments.is_empty());
        assert!(
            engine.message.contains("can't paste an image"),
            "message: {}",
            engine.message
        );
    }

    /// RED verified: with the `!self.acp_prompt_capabilities.image` check
    /// removed, this fails — a pasted image attaches even though the agent
    /// never declared `promptCapabilities.image`, same trap as
    /// `ai_attach_file_refuses_an_image_when_the_agent_lacks_the_capability`.
    #[test]
    fn paste_clipboard_image_refuses_when_the_agent_lacks_the_capability() {
        let mut engine = Engine::new_for_test();
        assert!(!engine.acp_prompt_capabilities.image);
        engine.clipboard_read_image = Some(Box::new(|| Ok(small_rgba_image())));

        engine.acp_attach_clipboard_image();

        assert!(engine.acp_manual_attachments.is_empty());
        assert!(
            engine.message.contains("doesn't support image attachments"),
            "message: {}",
            engine.message
        );
    }

    /// The backend reports "no image on the clipboard right now" via
    /// `BackendError::Unsupported` (same variant `Clipboard::read_image`
    /// itself returns for that case) — must surface the same clear "can't
    /// paste here" message as a platform with no callback at all, not a
    /// raw `{e:?}` dump.
    #[test]
    fn paste_clipboard_image_reports_unsupported_as_a_clear_refusal() {
        let mut engine = Engine::new_for_test();
        engine.acp_prompt_capabilities.image = true;
        engine.clipboard_read_image = Some(Box::new(|| Err(quadraui::BackendError::Unsupported)));

        engine.acp_attach_clipboard_image();

        assert!(engine.acp_manual_attachments.is_empty());
        assert!(
            engine.message.contains("can't paste an image"),
            "message: {}",
            engine.message
        );
    }

    /// Any other backend error (a genuine platform failure, not "nothing to
    /// paste") must still surface *some* message rather than being
    /// swallowed — distinct from the `Unsupported` case above.
    #[test]
    fn paste_clipboard_image_surfaces_other_backend_errors() {
        let mut engine = Engine::new_for_test();
        engine.acp_prompt_capabilities.image = true;
        engine.clipboard_read_image = Some(Box::new(|| {
            Err(quadraui::BackendError::PlatformFailure {
                context: "test failure".to_string(),
            })
        }));

        engine.acp_attach_clipboard_image();

        assert!(engine.acp_manual_attachments.is_empty());
        assert!(
            engine.message.contains("Clipboard paste failed"),
            "message: {}",
            engine.message
        );
    }

    /// RED verified: with `Engine::acp_attach_clipboard_image`'s size check
    /// against the PNG-*encoded* length removed, this fails — a large image
    /// attaches instead of being refused.
    #[test]
    fn paste_clipboard_image_rejects_an_oversize_image() {
        let mut engine = Engine::new_for_test();
        engine.acp_prompt_capabilities.image = true;
        engine.clipboard_read_image = Some(Box::new(|| Ok(oversize_rgba_image())));

        engine.acp_attach_clipboard_image();

        assert!(
            engine.acp_manual_attachments.is_empty(),
            "oversize clipboard image must not be staged: {:?}",
            engine.acp_manual_attachments
        );
        assert!(
            engine.message.contains("larger than the 5.0 MB limit"),
            "message: {}",
            engine.message
        );
    }

    /// The issue's own acceptance line applied to the clipboard-paste half:
    /// "Assert an image block is sent with the right mime and base64 when
    /// the capability is present" — stages the attachment with the exact
    /// PNG bytes `encode_png_rgba8` would produce for the mocked image.
    ///
    /// RED verified: with `AcpManualAttachment::Image`'s push in
    /// `Engine::acp_attach_clipboard_image` stubbed to a no-op, this fails —
    /// `acp_manual_attachments` stays empty after a successful paste.
    #[test]
    fn paste_clipboard_image_stages_a_png_attachment_when_the_capability_is_present() {
        let mut engine = Engine::new_for_test();
        engine.acp_prompt_capabilities.image = true;
        let image = small_rgba_image();
        let expected_png =
            crate::core::acp::encode_png_rgba8(image.width, image.height, &image.pixels)
                .expect("encode should succeed");
        engine.clipboard_read_image = Some(Box::new(|| Ok(small_rgba_image())));

        engine.acp_attach_clipboard_image();

        assert_eq!(
            engine.acp_manual_attachments.len(),
            1,
            "{:?}",
            engine.message
        );
        let block = engine.acp_manual_attachments[0].content_block();
        assert_eq!(block["type"], "image");
        assert_eq!(block["mimeType"], "image/png");
        use base64::Engine as _;
        assert_eq!(
            block["data"],
            base64::engine::general_purpose::STANDARD.encode(&expected_png)
        );
        assert!(
            engine.message.contains("Attached clipboard image"),
            "message: {}",
            engine.message
        );
    }

    // ── ACP-2 (#953): session/request_permission human-in-the-loop dialog ──

    /// Poll `engine` until it has a dialog open tagged `"acp_permission"`.
    #[cfg(unix)]
    fn poll_until_permission_dialog(engine: &mut Engine) {
        poll_acp_until(engine, |e| {
            e.dialog.as_ref().is_some_and(|d| d.tag == "acp_permission")
        });
    }

    /// A `session/request_permission` request must open the
    /// `"acp_permission"` dialog with the tool call's `title`/`kind`/
    /// `locations` actually rendered into it — not just some state flag
    /// flipped. "A permission prompt with no visible target is not a
    /// decision, it is a rubber stamp" (#953's acceptance bar).
    #[cfg(unix)]
    #[test]
    fn request_permission_opens_dialog_with_tool_title_kind_and_locations() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_REQUEST_PERMISSION", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();

        engine.ai_send_message("please edit".to_string());
        poll_until_permission_dialog(&mut engine);

        let dialog = engine
            .dialog
            .as_ref()
            .expect("permission dialog should be open");
        assert_eq!(dialog.tag, "acp_permission");
        assert_eq!(dialog.title, "Edit src/main.rs");
        let body = dialog.body.join("\n");
        assert!(
            body.contains("edit"),
            "the tool-call kind must be rendered so a human has a category \
             to decide on: {body:?}"
        );
        assert!(
            body.contains("src/main.rs:42"),
            "the tool-call location (path + line) must be rendered: {body:?}"
        );
        let labels: Vec<&str> = dialog.buttons.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["Allow Once", "Always Allow", "Reject"],
            "the agent's own options must be presented verbatim, not a \
             hardcoded yes/no"
        );
        assert!(
            engine.acp_pending_permission.is_some(),
            "the request must be tracked as parked while its dialog is open"
        );
    }

    /// Selecting an option replies with that option's `optionId` and lets
    /// the turn resume to completion — the reply actually reaches the
    /// (fake) agent, which was blocked on it.
    ///
    /// RED verified: with the `"acp_permission"` arm of
    /// `process_dialog_result` deleted (falling through to the `_ =>
    /// EngineAction::None` default, which never calls
    /// `respond_to_client_request`), this test times out instead of
    /// observing `ai_streaming` clear, because the fixture stays blocked on
    /// its `read -r _reply` forever.
    #[cfg(unix)]
    #[test]
    fn selecting_an_option_replies_and_resumes_the_turn() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_REQUEST_PERMISSION", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();

        engine.ai_send_message("please edit".to_string());
        poll_until_permission_dialog(&mut engine);

        let idx = engine
            .dialog
            .as_ref()
            .unwrap()
            .buttons
            .iter()
            .position(|b| b.label == "Allow Once")
            .expect("Allow Once button should be present");
        engine.dialog_click_button(idx);

        assert!(
            engine.dialog.is_none(),
            "the dialog must close the moment a button is clicked"
        );
        assert!(
            engine.acp_pending_permission.is_none(),
            "answering the request must clear the parked-request bookkeeping"
        );

        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(
            !engine.ai_streaming,
            "the turn must reach stopReason: end_turn within {TEST_DEADLINE:?} \
             once the reply unblocks the fixture's read"
        );
    }

    /// #953: "Esc-dismiss replies cancelled and the turn ends cleanly
    /// rather than hanging." `dialog_cancel()` is the engine-level
    /// equivalent of the in-canvas Escape key / GTK native dialog's
    /// dismiss-without-a-button path (`panels.rs`'s `dialog_cancel` doc).
    ///
    /// RED verified the same way as the sibling test above: without the
    /// `"acp_permission"` arm, `dialog_cancel()`'s call into
    /// `process_dialog_result` never answers the parked request and this
    /// test times out waiting for `ai_streaming` to clear.
    #[cfg(unix)]
    #[test]
    fn escape_dismiss_replies_cancelled_and_the_turn_ends_cleanly() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_REQUEST_PERMISSION", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();

        engine.ai_send_message("please edit".to_string());
        poll_until_permission_dialog(&mut engine);

        engine.dialog_cancel();
        assert!(engine.dialog.is_none());
        assert!(engine.acp_pending_permission.is_none());

        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(
            !engine.ai_streaming,
            "a cancelled reply must still unblock the fixture and let the \
             turn end cleanly within {TEST_DEADLINE:?}, not hang forever"
        );
    }

    /// #953: "allow_always suppresses the second prompt for the same tool
    /// within the session, and not beyond it." Two prompts in the *same*
    /// session — the second must complete without ever reopening the
    /// dialog, because the fake agent still blocks on a reply each time
    /// (see the fixture's doc comment), so a hang here means the engine
    /// silently dropped the second request instead of answering it from
    /// the remembered decision.
    #[cfg(unix)]
    #[test]
    fn allow_always_suppresses_the_second_prompt_within_the_session() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_REQUEST_PERMISSION", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();

        engine.ai_send_message("first edit".to_string());
        poll_until_permission_dialog(&mut engine);
        let idx = engine
            .dialog
            .as_ref()
            .unwrap()
            .buttons
            .iter()
            .position(|b| b.label == "Always Allow")
            .expect("Always Allow button should be present");
        engine.dialog_click_button(idx);
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(!engine.ai_streaming, "first turn should complete");
        assert_eq!(
            engine.acp_remembered_decisions.get("edit"),
            Some(&true),
            "picking Always Allow must remember the decision, keyed by the \
             tool-call kind"
        );

        // Second turn, same session: the fixture will emit another
        // session/request_permission and block on its reply exactly like
        // the first time — but the dialog must never reopen.
        engine.ai_send_message("second edit".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(
            !engine.ai_streaming,
            "the second turn must complete within {TEST_DEADLINE:?} — a \
             hang here means the remembered decision wasn't applied and \
             the fixture is still blocked waiting for a reply"
        );
        assert!(
            engine.dialog.is_none(),
            "the second permission prompt for the same tool kind must be \
             auto-answered, never shown"
        );
    }

    /// #953: "Agent death with a dialog open leaves no dialog on screen and
    /// no write to a dead pipe." `poll_acp`'s `AgentExited` arm clears the
    /// dialog directly rather than routing through
    /// `acp_cancel_pending_permission` (which would call
    /// `respond_to_client_request`, i.e. write to the now-dead child's
    /// stdin) — this test proves the *observable* half (dialog gone, no
    /// panic, clean state); the "doesn't write" half is structural (the
    /// `AgentExited` arm never touches `acp_client` to send anything, and
    /// `acp_client` is still `Some` — not yet dropped — for the duration of
    /// this arm, so a write attempt would have to be an explicit call this
    /// arm simply doesn't make).
    #[cfg(unix)]
    #[test]
    fn agent_death_with_permission_dialog_open_clears_dialog_cleanly() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_DIE_DURING_PERMISSION", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();

        engine.ai_send_message("please edit".to_string());

        // The fixture emits the request_permission request and exits in the
        // same breath (no blocking read in between, unlike every other
        // fixture branch) — same shape as `core::acp::tests::
        // agent_death_mid_session_surfaces_as_event_no_panic`'s
        // `ACP_FAKE_DIE_AFTER_INIT` case: the `ClientRequest` and the
        // `AgentExited` its EOF produces can land in the *same*
        // `AcpClient::poll()` drain, processed in order within the same
        // `poll_acp()` call, so a dialog-open checkpoint in between is not
        // reliably observable (see that test's doc for why asserting an
        // intermediate state here would be a scheduling-dependent flake,
        // not a real assertion). What's actually guaranteed — and what
        // #953 asks for — is the *end* state: drive polls until the death
        // is fully drained, then confirm nothing was left dangling.
        poll_acp_until(&mut engine, |e| e.acp_client.is_none());
        assert!(
            engine.acp_client.is_none(),
            "agent death should clear the client within {TEST_DEADLINE:?}"
        );
        assert!(
            engine.dialog.is_none(),
            "the permission dialog must not linger on screen once its \
             agent is gone"
        );
        assert!(
            engine.acp_pending_permission.is_none(),
            "the parked request must be dropped, not left to answer later"
        );
    }

    /// #953 review: "the user must be able to abort a running turn from the
    /// panel" — `dispatch_ai_chat_event`'s Ctrl+C wiring
    /// (`src/core/engine/ext_panel.rs`) must actually route through
    /// `acp_cancel_turn` while a live ACP turn is streaming, not just exist
    /// as an unreferenced method. Exercises it exactly the way the AI panel
    /// does: a `ChatControllerEvent::KeyPressed` for Ctrl+C dispatched while
    /// `acp_client.is_some() && ai_streaming` — with a permission dialog
    /// parked mid-turn, so this also covers "session cancelled while a
    /// prompt is open -> close the dialog, reply cancelled" (#953's
    /// acceptance bar).
    ///
    /// RED verified: with `acp_cancel_turn`'s body emptied to a no-op, this
    /// test fails outright (`ai_streaming` stays `true` and the dialog stays
    /// open immediately after dispatch, instead of clearing synchronously).
    #[cfg(unix)]
    #[test]
    fn ctrl_c_during_a_streaming_turn_cancels_via_acp_cancel_turn_not_ai_clear() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_REQUEST_PERMISSION", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();

        engine.ai_send_message("please edit".to_string());
        poll_until_permission_dialog(&mut engine);
        assert!(
            engine.ai_streaming,
            "sanity: the turn must still be streaming/parked before Ctrl+C"
        );

        engine.dispatch_ai_chat_event(quadraui::ChatControllerEvent::KeyPressed {
            key: "Char('c')".to_string(),
            modifiers: quadraui::Modifiers {
                ctrl: true,
                ..Default::default()
            },
        });

        // `acp_cancel_turn` clears busy state synchronously — it must not
        // wait for a `PromptStopped` a hung/misbehaving agent might never
        // send.
        assert!(
            !engine.ai_streaming,
            "Ctrl+C must clear the busy state immediately, not wait for the \
             agent to acknowledge"
        );
        assert!(
            engine.dialog.is_none(),
            "the parked permission dialog must close the moment the turn is \
             cancelled"
        );
        assert!(
            engine.acp_pending_permission.is_none(),
            "the parked permission request must be answered (not left to \
             hang) as part of cancelling the turn"
        );
        assert!(
            engine
                .ai_messages
                .last()
                .is_some_and(|m| m.content.contains("cancelled by user")),
            "a cancellation notice should land in the transcript: {:?}",
            engine.ai_messages
        );
        // Ctrl+C's ACP-2 behaviour is scoped abort of the turn, NOT
        // `ai_clear`'s full teardown — the session/process must stay alive
        // so the user can send another message without re-spawning the
        // agent.
        assert!(
            engine.acp_client.is_some(),
            "cancelling a turn must not tear down the agent session — that \
             is ai_clear's job, not Ctrl+C's, while a turn is in flight"
        );
        assert!(
            !engine.ai_messages.is_empty(),
            "unlike ai_clear, cancelling an in-flight turn must not wipe \
             the transcript"
        );
    }

    /// #953 review: the *other* named reply path for a parked permission
    /// dialog — "dialog dismissed ... or by any unrelated action that
    /// closes it" — must also produce exactly one `cancelled` reply, not
    /// just the Esc/`dialog_cancel()` half already covered above.
    /// `show_dialog`'s own `acp_cancel_pending_permission()` guard
    /// (`src/core/engine/panels.rs`) is what makes this safe: opening any
    /// other dialog (here, the quit-confirmation prompt) while an
    /// `"acp_permission"` dialog is parked must answer it first.
    ///
    /// RED verified: deleting the `self.acp_cancel_pending_permission();`
    /// call at the top of `show_dialog` makes this test hang — the fixture
    /// stays blocked on its `read -r _reply` forever because nothing ever
    /// answers request 9002, even though a different dialog is now on
    /// screen.
    #[cfg(unix)]
    #[test]
    fn an_unrelated_dialog_replacing_the_permission_prompt_still_replies_cancelled() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_REQUEST_PERMISSION", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();

        engine.ai_send_message("please edit".to_string());
        poll_until_permission_dialog(&mut engine);
        assert!(engine.acp_pending_permission.is_some());

        // An unrelated event opens a completely different dialog over the
        // still-parked permission prompt — e.g. the app deciding to confirm
        // quitting with unsaved changes.
        engine.show_quit_confirm();

        assert_eq!(
            engine.dialog.as_ref().map(|d| d.tag.as_str()),
            Some("quit_unsaved"),
            "the unrelated dialog must actually take over the screen"
        );
        assert!(
            engine.acp_pending_permission.is_none(),
            "the replaced permission request must be answered (cancelled), \
             not silently dropped or left parked behind the new dialog"
        );

        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(
            !engine.ai_streaming,
            "the cancelled reply must actually reach the (fake) agent and \
             let the turn end cleanly within {TEST_DEADLINE:?}, not hang \
             forever behind the unrelated dialog"
        );
    }

    /// #953 review (non-blocking concern): the malformed-request error path
    /// — "a JSON-RPC error, since there is nothing a human could
    /// meaningfully select" — must actually reach the (fake) agent exactly
    /// like every other reply path in this file, and must never open a
    /// dialog for a request with no selectable options.
    #[cfg(unix)]
    #[test]
    fn malformed_request_permission_replies_with_an_error_not_a_dialog() {
        let mut engine =
            engine_with_fixture_agent(&[("ACP_FAKE_MALFORMED_REQUEST_PERMISSION", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();

        engine.ai_send_message("please edit".to_string());

        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(
            !engine.ai_streaming,
            "the error reply must actually reach the (fake) agent and let \
             the turn end cleanly within {TEST_DEADLINE:?}, not hang \
             forever waiting for a dialog that never opens"
        );
        assert!(
            engine.dialog.is_none(),
            "a request with no options array has nothing a human could \
             select — it must never open a dialog"
        );
        assert!(
            engine.acp_pending_permission.is_none(),
            "a malformed request must never be tracked as parked"
        );
    }

    // ── fs/read_text_file, fs/write_text_file (#954, ACP-3) ────────────────

    fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "acp3-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // #1374: canonicalize before handing back. On macOS `std::env::
        // temp_dir()` is `/var/folders/...`, and `/var` is a symlink to
        // `/private/var` — but `Engine::acp_write_text_file`'s
        // `resolve_path_within_roots` canonicalizes both the workspace
        // root and the resolved target path before opening the buffer, so
        // the buffer ends up keyed by the `/private/var/...` form while a
        // caller comparing against this un-canonicalized `dir`-derived
        // path (e.g. `buffer_manager.get(id).file_path == Some(file_path.
        // as_path())`, an exact-path comparison, not the canonicalizing
        // one `BufferManager::open_file`/`Engine::acp_read_text_file` use
        // for their own buffer lookups) never matches on that platform —
        // same class of bug as #1350. Canonicalizing here once, up front,
        // makes every path this helper hands out agree with what
        // `resolve_path_within_roots` resolves to, on every platform.
        dir.canonicalize().unwrap_or(dir)
    }

    /// #954's core correctness property: a dirty (unsaved) open buffer's
    /// in-memory content must win over disk when the agent calls
    /// `fs/read_text_file` — reading from disk instead makes the agent
    /// reason about stale text and propose edits against lines the user
    /// already changed. Drives the real dispatch path (`poll_acp` ->
    /// `Engine::acp_handle_read_text_file` -> `Engine::acp_read_text_file`)
    /// against the fixture's `ACP_FAKE_FS_READ_PATH` branch, which echoes
    /// whatever content it got back into the transcript — so this reads the
    /// answer off `ai_messages`, not off internal engine state.
    ///
    /// RED verified: reverting `acp_read_text_file` to always
    /// `std::fs::read_to_string` (skip the buffer-first lookup) makes this
    /// fail — the transcript shows `"read:on disk"` instead of
    /// `"read:dirty in memory"`.
    #[cfg(unix)]
    #[test]
    fn fs_read_text_file_serves_dirty_buffer_content_not_disk() {
        let dir = unique_temp_dir("read");
        let file_path = dir.join("dirty.txt");
        std::fs::write(&file_path, "on disk").unwrap();
        let path_str = file_path.to_string_lossy().into_owned();

        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_FS_READ_PATH", &path_str)]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.workspace_root = Some(dir.clone());

        let buffer_id = engine
            .buffer_manager
            .open_file(&file_path)
            .expect("should open");
        {
            let state = engine.buffer_manager.get_mut(buffer_id).unwrap();
            let len = state.buffer.content.len_chars();
            state.buffer.content.remove(0..len);
            state.buffer.content.insert(0, "dirty in memory");
            state.dirty = true;
        }

        engine.ai_send_message("please read".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(
            !engine.ai_streaming,
            "turn should complete within {TEST_DEADLINE:?}"
        );

        let transcript: Vec<&str> = engine
            .ai_messages
            .iter()
            .map(|m| m.content.as_str())
            .collect();
        assert!(
            transcript
                .iter()
                .any(|c| c.contains("read:dirty in memory")),
            "expected the dirty buffer's in-memory content in the \
             transcript: {transcript:?}"
        );
        assert!(
            !transcript.iter().any(|c| c.contains("read:on disk")),
            "must never serve stale on-disk content for an open, dirty \
             buffer: {transcript:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `fs/read_text_file`'s `line`/`limit` must actually reach
    /// `select_text_lines` through the real dispatch method (not just be
    /// unit-tested in isolation on `core::acp`) — a closed file, so this
    /// also exercises the plain-disk-read fallback half of buffer-first
    /// resolution.
    #[test]
    fn acp_read_text_file_honours_line_and_limit_against_a_closed_file() {
        let dir = unique_temp_dir("read-line-limit");
        let file_path = dir.join("lines.txt");
        std::fs::write(&file_path, "one\ntwo\nthree\nfour").unwrap();

        let engine = Engine::new_for_test();
        let result = engine
            .acp_read_text_file(&file_path, Some(2), Some(2))
            .expect("closed file should be readable from disk");
        assert_eq!(result, "two\nthree");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #954: "`fs/write_text_file` on a closed file -> file opens, edit
    /// applies, a single undo reverts it." Drives the real dispatch path
    /// (`poll_acp` -> `Engine::acp_handle_write_text_file` ->
    /// `Engine::acp_write_text_file`) against the fixture's
    /// `ACP_FAKE_FS_WRITE_PATH` branch, which reports back `"write:ok"` or
    /// `"write:error:..."` depending on whether the reply was a JSON-RPC
    /// error, so the reply itself is observed through the transcript, not
    /// just trusted.
    ///
    /// RED verified: reverting `apply_workspace_edit`'s pre-#954
    /// closed-file shape onto this call path (raw `fs::write`, no buffer,
    /// no undo group) makes the final `state.undo()` assertion fail outright
    /// (no buffer ever exists to undo) instead of reverting the content.
    #[cfg(unix)]
    #[test]
    fn fs_write_text_file_on_closed_file_opens_buffer_applies_and_a_single_undo_reverts_it() {
        let dir = unique_temp_dir("write-closed");
        let file_path = dir.join("closed.txt");
        std::fs::write(&file_path, "original content").unwrap();
        let path_str = file_path.to_string_lossy().into_owned();

        let mut engine = engine_with_fixture_agent(&[
            ("ACP_FAKE_FS_WRITE_PATH", &path_str),
            ("ACP_FAKE_FS_WRITE_CONTENT", "new content from agent"),
        ]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.workspace_root = Some(dir.clone());
        assert!(
            !engine.buffer_manager.is_path_open(&file_path),
            "sanity: the file must start closed"
        );

        engine.ai_send_message("please write".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(
            !engine.ai_streaming,
            "turn should complete within {TEST_DEADLINE:?}"
        );

        let transcript: Vec<&str> = engine
            .ai_messages
            .iter()
            .map(|m| m.content.as_str())
            .collect();
        assert!(
            transcript.iter().any(|c| c.contains("write:ok")),
            "the write must succeed and be reported ok: {transcript:?}"
        );

        assert!(
            engine.buffer_manager.is_path_open(&file_path),
            "fs/write_text_file on a closed file must open it into a buffer"
        );
        let buffer_id = engine
            .buffer_manager
            .list()
            .into_iter()
            .find(|&id| {
                engine
                    .buffer_manager
                    .get(id)
                    .and_then(|s| s.file_path.as_deref())
                    == Some(file_path.as_path())
            })
            .expect("buffer for the written path should exist");

        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "new content from agent",
            "fs/write_text_file must persist to disk"
        );
        assert_eq!(
            engine
                .buffer_manager
                .get(buffer_id)
                .unwrap()
                .buffer
                .content
                .to_string(),
            "new content from agent"
        );

        let state = engine.buffer_manager.get_mut(buffer_id).unwrap();
        assert!(
            state.undo().is_some(),
            "a single undo must be available to revert the agent's write"
        );
        assert_eq!(
            state.buffer.content.to_string(),
            "original content",
            "one undo must revert the whole write, like any other edit"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #954: "Write to a new path inside cwd creates the file" + "a single
    /// undo reverts it" for the brand-new-file case too (no prior on-disk
    /// content to fall back to).
    #[test]
    fn acp_write_text_file_creates_a_new_file_and_a_single_undo_reverts_it() {
        let dir = unique_temp_dir("write-new");
        let file_path = dir.join("brand-new.txt");
        assert!(!file_path.exists(), "sanity: must not exist yet");

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());

        engine
            .acp_write_text_file(&file_path, "hello from the agent")
            .expect("writing a new file inside the workspace root should succeed");

        assert!(
            file_path.exists(),
            "fs/write_text_file must create the file"
        );
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "hello from the agent"
        );

        let buffer_id = engine
            .buffer_manager
            .list()
            .into_iter()
            .find(|&id| {
                engine
                    .buffer_manager
                    .get(id)
                    .and_then(|s| s.file_path.as_deref())
                    == Some(file_path.as_path())
            })
            .expect("the new file must be open in a buffer");
        let state = engine.buffer_manager.get_mut(buffer_id).unwrap();
        assert!(state.undo().is_some());
        assert_eq!(
            state.buffer.content.to_string(),
            "",
            "undoing a brand-new file's only edit must revert to empty content"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #954: "write outside cwd is refused." No JSON-RPC round trip needed
    /// here — this is `Engine::acp_write_text_file`'s own contract, and the
    /// fixture round trip above already proves the reply mechanism works.
    #[test]
    fn acp_write_text_file_rejects_a_path_outside_the_workspace_root() {
        let dir = unique_temp_dir("write-root");
        let outside = unique_temp_dir("write-outside");
        let target = outside.join("should-not-be-written.txt");

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());

        let result = engine.acp_write_text_file(&target, "malicious content");
        assert!(
            result.is_err(),
            "a write outside the workspace root must be refused"
        );
        assert!(
            !target.exists(),
            "a refused write must never touch the filesystem"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// #954: "Write error (read-only path) surfaces to the user and returns
    /// a JSON-RPC error, not a swallowed failure." Skips itself when running
    /// as root (common in some CI containers) — root bypasses Unix
    /// permission bits entirely, so the write would simply succeed and the
    /// scenario this test exists to cover (a *legitimate* disk failure
    /// reaching the caller instead of being swallowed) can't be produced
    /// this way in that environment.
    #[cfg(unix)]
    #[test]
    fn acp_write_text_file_surfaces_a_disk_write_error_not_swallowed() {
        use std::os::unix::fs::PermissionsExt;

        if unsafe { libc::geteuid() } == 0 {
            eprintln!(
                "skipping acp_write_text_file_surfaces_a_disk_write_error_not_swallowed: \
                 running as root, permission bits don't block writes"
            );
            return;
        }

        let dir = unique_temp_dir("write-readonly");
        let file_path = dir.join("readonly.txt");
        std::fs::write(&file_path, "orig").unwrap();
        let mut perms = std::fs::metadata(&file_path).unwrap().permissions();
        perms.set_mode(0o444);
        std::fs::set_permissions(&file_path, perms).unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());

        let result = engine.acp_write_text_file(&file_path, "new content");
        assert!(
            result.is_err(),
            "a disk write failure must surface as an Err, not be swallowed"
        );
        assert!(
            !result.unwrap_err().is_empty(),
            "the error must actually name something, not just be a bare failure marker"
        );
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "orig",
            "a failed write must not have partially clobbered the file"
        );

        // Restore write permission so the temp dir can be cleaned up.
        let mut perms = std::fs::metadata(&file_path).unwrap().permissions();
        perms.set_mode(0o644);
        let _ = std::fs::set_permissions(&file_path, perms);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── apply_workspace_edit closed-file regression (#954) ──────────────────

    /// The fix itself: a closed file targeted by a workspace edit (LSP
    /// rename-across-files, or a code action touching an unopened file)
    /// must be opened into a buffer and edited through the same
    /// undo-grouped path an already-open file gets — not written straight
    /// to disk with `fs::write` (no undo, errors swallowed — the pre-#954
    /// bug). This also proves the fix does *not* silently persist to disk
    /// on its own: the edit is left dirty for the user to review/save, the
    /// same as an edit to an already-open buffer would be.
    ///
    /// RED verified: this exact assertion sequence (buffer opens with the
    /// edit applied, disk untouched, one undo reverts it) cannot pass
    /// against the pre-fix code, which never created a buffer at all for a
    /// closed-file edit — there'd be nothing in `buffer_manager` to find.
    #[test]
    fn apply_workspace_edit_opens_closed_file_into_a_buffer_undo_grouped_no_auto_save() {
        let dir = unique_temp_dir("workspace-edit-closed");
        let file_path = dir.join("renamed.rs");
        std::fs::write(&file_path, "let old_name = 1;").unwrap();

        let mut engine = Engine::new_for_test();
        assert!(!engine.buffer_manager.is_path_open(&file_path));

        let edit = WorkspaceEdit {
            changes: vec![lsp::FileEdit {
                path: file_path.clone(),
                edits: vec![FormattingEdit {
                    range: lsp::LspRange {
                        start: lsp::LspPosition {
                            line: 0,
                            character: 4,
                        },
                        end: lsp::LspPosition {
                            line: 0,
                            character: 12,
                        },
                    },
                    new_text: "new_name".to_string(),
                }],
            }],
        };
        let errors = engine.apply_workspace_edit(edit);
        assert!(errors.is_empty(), "expected no errors: {errors:?}");

        assert!(
            engine.buffer_manager.is_path_open(&file_path),
            "the closed file must be opened into a buffer"
        );
        let buffer_id = engine
            .buffer_manager
            .list()
            .into_iter()
            .find(|&id| {
                engine
                    .buffer_manager
                    .get(id)
                    .and_then(|s| s.file_path.as_deref())
                    == Some(file_path.as_path())
            })
            .unwrap();
        let state = engine.buffer_manager.get(buffer_id).unwrap();
        assert_eq!(state.buffer.content.to_string(), "let new_name = 1;");
        assert!(
            state.dirty,
            "the edit must be left dirty, like any other edit"
        );
        assert_eq!(
            std::fs::read_to_string(&file_path).unwrap(),
            "let old_name = 1;",
            "must NOT silently write to disk — that's the exact behaviour \
             being replaced, just via a safer (buffer-backed) mechanism \
             instead of a raw fs::write"
        );

        let state = engine.buffer_manager.get_mut(buffer_id).unwrap();
        assert!(state.undo().is_some());
        assert_eq!(state.buffer.content.to_string(), "let old_name = 1;");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The other half of the regression: the pre-#954 closed-file branch
    /// used `if let Ok(text) = fs::read_to_string(...)`, so a failure (a
    /// path that can't be read as text — here, a directory) was silently
    /// dropped with no error and no buffer. `apply_workspace_edit` must
    /// surface it instead.
    #[test]
    fn apply_workspace_edit_surfaces_the_previously_swallowed_open_error() {
        let dir = unique_temp_dir("workspace-edit-error");
        // A directory can't be opened as a text buffer — `Buffer::from_file`
        // fails on it deterministically, independent of Unix permission
        // bits or the test's effective uid.
        let bad_path = dir.join("not_a_file");
        std::fs::create_dir_all(&bad_path).unwrap();

        let mut engine = Engine::new_for_test();
        let edit = WorkspaceEdit {
            changes: vec![lsp::FileEdit {
                path: bad_path.clone(),
                edits: vec![FormattingEdit {
                    range: lsp::LspRange::default(),
                    new_text: "x".to_string(),
                }],
            }],
        };
        let errors = engine.apply_workspace_edit(edit);
        assert_eq!(
            errors.len(),
            1,
            "the open failure must be surfaced, not swallowed: {errors:?}"
        );
        assert!(
            errors[0].contains(&bad_path.display().to_string()),
            "the error should name the offending path: {:?}",
            errors[0]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #954: path safety must reject writes escaping the workspace root even
    /// via `..` traversal, not just a plain "different directory" check —
    /// see `core::acp::resolve_path_within_roots`'s own dedicated tests for
    /// the pure-function coverage; this confirms `Engine::
    /// acp_write_text_file` actually calls it rather than some looser
    /// ad hoc check.
    #[test]
    fn acp_write_text_file_rejects_dot_dot_traversal_out_of_the_workspace_root() {
        let dir = unique_temp_dir("write-traversal-root");
        let sibling = unique_temp_dir("write-traversal-sibling");
        let escaping = dir.join("..").join(
            sibling
                .file_name()
                .expect("temp dir should have a file name")
                .to_owned(),
        );
        let target = escaping.join("escaped.txt");

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());

        let result = engine.acp_write_text_file(&target, "should never land");
        assert!(result.is_err(), "`..` must not escape the workspace root");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&sibling);
    }

    /// #956 (ACP-5): `:AiMode` (no argument) must summarise the agent's
    /// declared modes and mark which one is current — engine-level coverage
    /// underneath the driver-tier round-trip test
    /// (`gtk::testing::sidebar_panel_clicks::
    /// ai_panel_mode_switch_round_trips_via_session_set_mode`), which
    /// covers `:AiMode <target>` actually reaching the wire.
    #[cfg(unix)]
    #[test]
    fn ai_mode_no_arg_lists_modes_and_marks_current() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_SESSION_MODES", "1")]);
        // `Engine::poll_acp`'s `Initialized` handler drives `session/new`
        // itself once the (already-sent) `initialize` response lands.
        poll_acp_until(&mut engine, |e| e.acp_current_mode_id.is_some());

        assert_eq!(engine.acp_current_mode_id.as_deref(), Some("code"));
        engine.execute_command("AiMode");
        assert_eq!(engine.message, "Modes: *Code, Plan");
    }

    /// An unknown mode name/id must be rejected with a clear message, never
    /// silently sent to the agent as-is.
    #[test]
    fn ai_mode_unknown_target_is_rejected_without_a_client() {
        let mut engine = Engine::new_for_test();
        engine.execute_command("AiMode nonexistent");
        assert_eq!(engine.message, "No active ACP session");
    }

    // ── #957 (ACP-6): auth.terminal — subscription login ─────────────────

    /// `settings.acp_agent_command` pointing at the fixture agent — a
    /// *shell command string*, not an argv, because that is what
    /// `Engine::acp_launch_terminal_login` injects verbatim into the login
    /// pane's interactive shell.
    ///
    /// The fixture path is double-quoted because `CARGO_MANIFEST_DIR` is
    /// not guaranteed to be space-free: a checkout can live under a path
    /// like `~/Library/Application Support/...` on macOS, and unquoted
    /// the login shell word-splits that into `sh /Users/…/Library/Application`
    /// and exits 127 ("command not found") before the fixture ever runs —
    /// so the login reads as *failed* and `acp_authenticated` never flips.
    /// Double quotes are understood by both consumers of this string: POSIX
    /// shells (the login pane) and `core::acp::parse_agent_command` (the
    /// argv splitter). Same class of checkout-shaped fixture bug as #1350
    /// and #1374, and likewise test-only — a real user's
    /// `acp_agent_command` is their own shell string to quote.
    fn acp6_fixture_argv_string() -> String {
        format!(
            "sh \"{}\"",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            )
        )
    }

    fn poll_until_auth_choice_dialog(engine: &mut Engine) {
        poll_acp_until(engine, |e| {
            e.dialog
                .as_ref()
                .is_some_and(|d| d.tag == "acp_auth_choice")
        });
    }

    /// The gate itself, at the engine level: every pre-#957 fixture
    /// response (`authMethods: []`, the default with no extra env) must
    /// proceed straight to `session/new` exactly as before — no dialog, no
    /// behavior change for every agent that doesn't advertise auth. This is
    /// the engine-level half of "with the capability not advertised, the
    /// method is absent, confirming the gate is real" (the wire-level half
    /// — the fixture only offers a `type: "terminal"` entry when it
    /// actually saw `auth.terminal: true` on the request — lives in
    /// `core::acp`'s own tests).
    #[cfg(unix)]
    #[test]
    fn no_auth_methods_skips_the_dialog_same_as_before_957() {
        let mut engine = engine_with_fixture_agent(&[]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.ai_send_message("hello agent".to_string());
        poll_acp_until(&mut engine, |e| e.acp_session_id.is_some());
        assert!(
            engine.dialog.is_none(),
            "an agent with no authMethods must never see an auth dialog"
        );
        assert_eq!(engine.acp_session_id.as_deref(), Some("sess-1"));
    }

    /// An agent that offers `authMethods` must present the choice — with
    /// both the agent-kind and terminal-kind methods rendered as their own
    /// buttons — instead of silently opening a session.
    ///
    /// RED verified: with the `!self.acp_auth_methods.is_empty()` branch of
    /// `poll_acp`'s `Initialized` handler deleted (always calling
    /// `acp_begin_session()` unconditionally, i.e. #957 reverted), this
    /// test fails — `engine.dialog` stays `None` and `acp_session_id`
    /// becomes `Some("sess-1")` immediately instead.
    #[cfg(unix)]
    #[test]
    fn auth_methods_present_opens_choice_dialog_before_session_new() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_AUTH_METHODS", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.ai_send_message("hello agent".to_string());
        poll_until_auth_choice_dialog(&mut engine);

        assert!(
            engine.acp_session_id.is_none(),
            "session/new must wait for the auth choice, not fire immediately"
        );
        let dialog = engine.dialog.as_ref().unwrap();
        let actions: Vec<&str> = dialog.buttons.iter().map(|b| b.action.as_str()).collect();
        assert_eq!(
            actions,
            vec!["api-key", "claude-ai-login", "acp_auth_skip"],
            "both auth methods plus the skip fallback must be offered: {actions:?}"
        );
    }

    /// "Continue without auth" must resume the handshake unauthenticated —
    /// an agent advertising `authMethods` doesn't necessarily mean auth is
    /// required right now.
    #[cfg(unix)]
    #[test]
    fn skipping_auth_choice_proceeds_to_session_new() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_AUTH_METHODS", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.ai_send_message("hello agent".to_string());
        poll_until_auth_choice_dialog(&mut engine);

        let idx = engine
            .dialog
            .as_ref()
            .unwrap()
            .buttons
            .iter()
            .position(|b| b.action == "acp_auth_skip")
            .expect("skip button should be present");
        engine.dialog_click_button(idx);

        assert!(engine.dialog.is_none());
        poll_acp_until(&mut engine, |e| e.acp_session_id.is_some());
        assert_eq!(engine.acp_session_id.as_deref(), Some("sess-1"));
        assert!(engine.acp_authenticated);
    }

    /// A `type: "agent"` auth method calls the plain `authenticate` RPC and,
    /// on success, resumes the handshake — the turn the user's message
    /// started actually completes end to end.
    #[cfg(unix)]
    #[test]
    fn agent_auth_method_authenticates_and_resumes_the_turn() {
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_AUTH_METHODS", "1")]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.ai_send_message("hello agent".to_string());
        poll_until_auth_choice_dialog(&mut engine);

        let idx = engine
            .dialog
            .as_ref()
            .unwrap()
            .buttons
            .iter()
            .position(|b| b.action == "api-key")
            .expect("API Key button should be present");
        engine.dialog_click_button(idx);

        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(engine.acp_authenticated);
        assert_eq!(engine.acp_session_id.as_deref(), Some("sess-1"));
        let roles: Vec<&str> = engine.ai_messages.iter().map(|m| m.role.as_str()).collect();
        assert!(
            roles.contains(&"assistant"),
            "the turn queued before auth should resume and complete: {:?}",
            engine.ai_messages
        );
    }

    /// A failed `authenticate` call must leave the panel usable — clear the
    /// busy state and surface a message — never wedge it, matching the
    /// established `RequestFailed` contract (#952 review) this reuses
    /// unmodified.
    #[cfg(unix)]
    #[test]
    fn agent_auth_method_failure_leaves_panel_usable() {
        let mut engine = engine_with_fixture_agent(&[
            ("ACP_FAKE_AUTH_METHODS", "1"),
            ("ACP_FAKE_AUTH_FAIL", "1"),
        ]);
        engine.settings.acp_agent_command = "already-spawned-above".to_string();
        engine.ai_send_message("hello agent".to_string());
        poll_until_auth_choice_dialog(&mut engine);

        let idx = engine
            .dialog
            .as_ref()
            .unwrap()
            .buttons
            .iter()
            .position(|b| b.action == "api-key")
            .expect("API Key button should be present");
        engine.dialog_click_button(idx);

        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(!engine.ai_streaming);
        assert!(
            engine.message.contains("authenticate failed"),
            "expected a clear failure message: {:?}",
            engine.message
        );
        assert!(
            engine
                .ai_messages
                .iter()
                .any(|m| m.content.contains("authenticate")),
            "the failure should land in the transcript: {:?}",
            engine.ai_messages
        );
    }

    /// Choosing a `type: "terminal"` method launches the agent's own
    /// command interactively in a visible terminal pane; on a successful
    /// login (exit 0) the client re-initializes and resumes the queued
    /// turn — the full round trip the issue's acceptance bar describes.
    ///
    /// RED verified: with `Engine::acp_finish_terminal_login`'s `Some(0)`
    /// branch changed to a no-op (never calling `client.initialize()`),
    /// this test times out waiting for `acp_session_id` — the client stays
    /// parked after the login pane exits instead of resuming.
    #[cfg(unix)]
    #[test]
    fn terminal_auth_method_launches_login_and_resumes_on_success() {
        ensure_no_zsh_newuser_wizard();
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_AUTH_METHODS", "1")]);
        engine.settings.acp_agent_command = acp6_fixture_argv_string();
        engine.ai_send_message("hello agent".to_string());
        poll_until_auth_choice_dialog(&mut engine);

        let idx = engine
            .dialog
            .as_ref()
            .unwrap()
            .buttons
            .iter()
            .position(|b| b.action == "claude-ai-login")
            .expect("Claude Subscription button should be present");
        engine.dialog_click_button(idx);

        assert_eq!(
            engine.terminal_panes.len(),
            1,
            "choosing a terminal method should open exactly one login pane"
        );
        assert!(engine.terminal_open);

        let start = std::time::Instant::now();
        while !engine.terminal_panes.is_empty() && start.elapsed() < TEST_DEADLINE {
            engine.poll_terminal();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            engine.terminal_panes.is_empty(),
            "the login pane should have exited (exit 0) and been reaped"
        );
        assert!(engine.acp_authenticated);

        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(!engine.ai_streaming);
        assert_eq!(engine.acp_session_id.as_deref(), Some("sess-1"));
        let roles: Vec<&str> = engine.ai_messages.iter().map(|m| m.role.as_str()).collect();
        assert!(
            roles.contains(&"assistant"),
            "the turn queued before login should resume and complete: {:?}",
            engine.ai_messages
        );
    }

    /// A failed interactive login (non-zero exit) must leave the panel
    /// usable with a clear message — not wedged, and not silently treated
    /// as success.
    #[cfg(unix)]
    #[test]
    fn terminal_auth_login_failure_leaves_panel_usable() {
        ensure_no_zsh_newuser_wizard();
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_AUTH_METHODS", "1")]);
        engine.settings.acp_agent_command = format!("{} fail", acp6_fixture_argv_string());
        engine.ai_send_message("hello agent".to_string());
        poll_until_auth_choice_dialog(&mut engine);

        let idx = engine
            .dialog
            .as_ref()
            .unwrap()
            .buttons
            .iter()
            .position(|b| b.action == "claude-ai-login")
            .expect("Claude Subscription button should be present");
        engine.dialog_click_button(idx);

        let start = std::time::Instant::now();
        while !engine.terminal_panes.is_empty() && start.elapsed() < TEST_DEADLINE {
            engine.poll_terminal();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(engine.terminal_panes.is_empty());
        assert!(!engine.acp_authenticated);
        assert!(
            !engine.ai_streaming,
            "a failed login must clear the busy state, not wedge the panel"
        );
        assert!(
            engine.message.contains("exited with code"),
            "expected a clear failure message: {:?}",
            engine.message
        );
        assert!(
            engine.acp_client.is_none(),
            "a failed login should drop the client so the next attempt \
             starts a clean handshake"
        );
    }

    /// Closing the login pane before it exits on its own — the user
    /// abandoning the login — must be treated the same as a failed login:
    /// panel left usable, clear message, no silent "authenticated" state.
    #[cfg(unix)]
    #[test]
    fn terminal_auth_login_abandoned_by_closing_pane_leaves_panel_usable() {
        ensure_no_zsh_newuser_wizard();
        let mut engine = engine_with_fixture_agent(&[("ACP_FAKE_AUTH_METHODS", "1")]);
        engine.settings.acp_agent_command = format!("{} hang", acp6_fixture_argv_string());
        engine.ai_send_message("hello agent".to_string());
        poll_until_auth_choice_dialog(&mut engine);

        let idx = engine
            .dialog
            .as_ref()
            .unwrap()
            .buttons
            .iter()
            .position(|b| b.action == "claude-ai-login")
            .expect("Claude Subscription button should be present");
        engine.dialog_click_button(idx);

        // Give the fixture a moment to actually enter its hang state (block
        // on `read`) before racing a close against it — otherwise this
        // would also pass if the pane merely exited fast on its own,
        // proving nothing about the *abandon* path specifically.
        std::thread::sleep(std::time::Duration::from_millis(200));
        engine.poll_terminal();
        assert_eq!(engine.terminal_panes.len(), 1);
        assert!(
            !engine.terminal_panes[0].session.is_exited(),
            "the fixture should still be hung waiting for input"
        );

        engine.terminal_close_active_tab();

        assert!(engine.terminal_panes.is_empty());
        assert!(!engine.acp_authenticated);
        assert!(!engine.ai_streaming);
        assert!(
            engine.message.contains("abandoned"),
            "expected a clear abandon message: {:?}",
            engine.message
        );
        assert!(engine.acp_client.is_none());
    }

    // ── acp_open_review_for_diffs fragment safety (#1454) ───────────────────

    fn diff_block(
        path: &str,
        old_text: Option<&str>,
        new_text: &str,
    ) -> crate::core::acp::AcpToolCallContentBlock {
        crate::core::acp::AcpToolCallContentBlock::Diff {
            path: path.to_string(),
            old_text: old_text.map(str::to_string),
            new_text: new_text.to_string(),
        }
    }

    /// The core data-loss fix at the `Engine` level (mirrors
    /// `core::review::tests::resolve_fragment_replaces_only_the_matched_region_within_a_larger_file`,
    /// exercised through the real entry point `acp_open_review_for_diffs`
    /// uses): a `diff` block whose `oldText`/`newText` are a *fragment* of a
    /// much larger file opens a review entry whose `old_text`/`new_text`
    /// are the WHOLE file, surrounding lines intact — never the bare
    /// fragment.
    ///
    /// RED against the bug this issue reports: before this fix,
    /// `acp_open_review_for_diffs` passed the wire `oldText`/`newText`
    /// straight through as `ProposedChange`, so this entry's `new_text`
    /// would have been the literal fragment `"new line\n"`, and accepting
    /// it would have truncated the file to just that line (2959 -> 16
    /// lines, per the reported repro) — this asserts the *entry itself*,
    /// before any accept, already carries the surrounding context.
    #[test]
    fn acp_open_review_for_diffs_resolves_a_fragment_against_the_whole_file() {
        let dir = unique_temp_dir("review-fragment");
        let target = dir.join("target.txt");
        std::fs::write(&target, "line1\nold line\nline3\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_open_review_for_diffs(&[diff_block(
            target.to_str().unwrap(),
            Some("old line\n"),
            "new line\n",
        )]);

        let review = engine
            .change_review
            .as_ref()
            .expect("an unambiguous fragment must open a review entry");
        let entry = review.current_entry().unwrap();
        assert_eq!(
            entry.change.old_text.as_deref(),
            Some("line1\nold line\nline3\n"),
            "old_text must be the whole current file, not the bare fragment"
        );
        assert_eq!(
            entry.change.new_text, "line1\nnew line\nline3\n",
            "new_text must preserve every surrounding line, not just the \
             edited fragment"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// "A duplicate `oldText` is refused" (#1454's acceptance bar): a
    /// fragment that occurs twice in the current file must never open a
    /// review entry (there is no single unambiguous place to apply it), and
    /// the refusal is surfaced via `Engine::message` rather than silently
    /// dropped. The file on disk is untouched — this never even reaches a
    /// write.
    #[test]
    fn acp_open_review_for_diffs_refuses_an_ambiguous_duplicate_fragment() {
        let dir = unique_temp_dir("review-ambiguous");
        let target = dir.join("target.txt");
        std::fs::write(&target, "old line\nold line\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_open_review_for_diffs(&[diff_block(
            target.to_str().unwrap(),
            Some("old line\n"),
            "new line\n",
        )]);

        assert!(
            engine.change_review.is_none(),
            "an ambiguous fragment must never open a review entry"
        );
        assert!(
            engine.message.contains("ambiguous") || engine.message.contains("refus"),
            "the refusal must be surfaced with a clear reason: {:?}",
            engine.message
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "old line\nold line\n",
            "a refused edit must never touch the filesystem"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Double-apply guard (#1454): if the file's current content no longer
    /// contains `oldText` but already contains `newText` exactly once, the
    /// edit already landed by some other path (the real Claude ACP
    /// adapter's own `fs/write_text_file` beating this `diff` *report* to
    /// the buffer, per the issue). This must not open a review entry
    /// (nothing to accept) and must not be reported as a scary "not found"
    /// failure.
    #[test]
    fn acp_open_review_for_diffs_treats_an_already_applied_edit_as_a_no_op() {
        let dir = unique_temp_dir("review-already-applied");
        let target = dir.join("target.txt");
        std::fs::write(&target, "line1\nnew line\nline3\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_open_review_for_diffs(&[diff_block(
            target.to_str().unwrap(),
            Some("old line\n"),
            "new line\n",
        )]);

        assert!(
            engine.change_review.is_none(),
            "an already-applied edit must not open a review entry"
        );
        assert!(
            engine.message.contains("already applied"),
            "expected an informational message, not a failure: {:?}",
            engine.message
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `diff` block whose fragment happens to be the *entire* current
    /// file (the fake-agent fixture's shape, and #955's original
    /// assumption) still resolves and opens a review entry — the fix is
    /// backward-compatible with a whole-file `diff` block.
    #[test]
    fn acp_open_review_for_diffs_still_handles_a_whole_file_fragment() {
        let dir = unique_temp_dir("review-whole-file");
        let target = dir.join("target.txt");
        std::fs::write(&target, "old line\n").unwrap();

        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(dir.clone());
        engine.acp_open_review_for_diffs(&[diff_block(
            target.to_str().unwrap(),
            Some("old line\n"),
            "new line\n",
        )]);

        let review = engine.change_review.as_ref().unwrap();
        let entry = review.current_entry().unwrap();
        assert_eq!(entry.change.old_text.as_deref(), Some("old line\n"));
        assert_eq!(entry.change.new_text, "new line\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `old_text: None` (a new file) passes `new_text` straight through
    /// with no fragment resolution attempted — there's no "current
    /// contents" to resolve a new file against.
    #[test]
    fn acp_open_review_for_diffs_passes_a_new_file_through_unresolved() {
        let mut engine = Engine::new_for_test();
        engine.workspace_root = Some(unique_temp_dir("review-new-file"));
        engine.acp_open_review_for_diffs(&[diff_block("brand/new.rs", None, "fn main() {}\n")]);

        let review = engine.change_review.as_ref().unwrap();
        let entry = review.current_entry().unwrap();
        assert_eq!(entry.change.old_text, None);
        assert_eq!(entry.change.new_text, "fn main() {}\n");
    }

    // ── #1459: session history / resume ─────────────────────────────────────

    /// An `Engine` configured with a `settings.acp_agents` registry entry
    /// naming this fixture, so `Engine::acp_resume_session`'s cold-start
    /// spawn path (and `ai_send_message_via_acp`'s) can launch a *fresh*
    /// subprocess through the real config path — unlike
    /// [`engine_with_fixture_agent`], which pre-spawns one client directly
    /// and cannot be used for a scenario that needs to spawn twice (once
    /// for the original session, once more for the resumed one, since
    /// `:AiClear` kills the first subprocess).
    #[cfg(unix)]
    fn engine_with_registered_fixture_agent(extra_env: &[(&str, &str)]) -> Engine {
        let fixture = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/fake_acp_agent.sh"
        );
        let mut engine = Engine::new_for_test();
        engine.settings.acp_agents = vec![crate::core::acp::AcpAgentProfile {
            name: "claude".to_string(),
            command: format!("sh \"{fixture}\""),
            cwd: String::new(),
            env: extra_env.iter().map(|(k, v)| format!("{k}={v}")).collect(),
        }];
        engine.settings.acp_active_agent = "claude".to_string();
        engine
    }

    /// The issue's core acceptance bar: resuming a past session rebuilds
    /// the transcript's user, assistant *and* tool-call turns through the
    /// same chunk/tool-call paths a live turn uses (asserted on
    /// `ai_messages`/`acp_tool_calls` content — the exact data
    /// `render::populate_ai_chat_controller` paints from, not a UI-layer
    /// flag), and the next prompt after resuming continues the *same*
    /// session id the picker resumed rather than silently starting a new
    /// one.
    ///
    /// RED verified: with `Engine::acp_begin_session`'s resume branch
    /// deleted, this fails at the first `poll_acp_until` past `ai_clear` —
    /// `acp_session_id` never becomes `Some("sess-1")` again because no
    /// `session/load` request is ever sent (a plain `session/new` would
    /// still succeed, just with a *different* fixture-assigned id in
    /// general — it only reads as "sess-1" here because this fixture
    /// always assigns that same literal id, which is exactly the kind of
    /// false-positive `record_session`'s "idempotent by id" test guards
    /// against on the index side).
    #[cfg(unix)]
    #[test]
    fn acp_resume_session_rebuilds_transcript_and_continues_the_same_session_id() {
        let mut engine = engine_with_registered_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_LOAD_SESSION", "1"),
        ]);

        engine.ai_send_message("remember this please".to_string());
        poll_acp_until(&mut engine, |e| e.acp_session_id.is_some());
        let first_session_id = engine
            .acp_session_id
            .clone()
            .expect("the handshake should have produced a session id");
        poll_acp_until(&mut engine, |e| !e.ai_streaming);

        engine.ai_clear();
        assert!(
            engine.acp_client.is_none(),
            ":AiClear must kill the live agent subprocess"
        );

        // The local index must still know about it, and must have learned
        // this agent supports resume.
        assert_eq!(
            engine.acp_session_index.load_session_capability("claude"),
            Some(true)
        );
        engine.acp_open_sessions_picker();
        assert!(
            engine.picker_open,
            "the picker must open for an agent that advertises loadSession"
        );
        assert_eq!(engine.picker_items.len(), 1);

        // Confirm the (only) entry.
        engine.picker_confirm();
        poll_acp_until(&mut engine, |e| {
            e.ai_messages
                .iter()
                .any(|m| m.content.contains("It prints hello."))
        });

        assert_eq!(
            engine.acp_session_id.as_deref(),
            Some(first_session_id.as_str()),
            "resuming must reuse the exact session id that was recorded, \
             not a freshly assigned one"
        );

        let roles: Vec<&str> = engine.ai_messages.iter().map(|m| m.role.as_str()).collect();
        assert!(
            roles.contains(&"user"),
            "the replayed user turn must rebuild: {:?}",
            engine.ai_messages
        );
        assert!(
            engine
                .ai_messages
                .iter()
                .any(|m| m.content.contains("what does main.rs do")),
            "the replayed user turn's text must survive: {:?}",
            engine.ai_messages
        );
        assert!(
            engine
                .ai_messages
                .iter()
                .any(|m| m.role == "assistant" && m.content.contains("It prints hello.")),
            "the replayed assistant turn must rebuild: {:?}",
            engine.ai_messages
        );
        assert!(
            engine
                .acp_tool_calls
                .iter()
                .any(|c| c.title == "Read README.md"),
            "the replayed tool-call turn must rebuild too, not just \
             message chunks: {:?}",
            engine.acp_tool_calls
        );

        // The resumed session must still answer a further prompt, under
        // the exact same session id.
        engine.ai_send_message("thanks".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert_eq!(
            engine.acp_session_id.as_deref(),
            Some(first_session_id.as_str()),
            "a further prompt after resuming must not change the session id"
        );
    }

    /// The other half of the issue's acceptance bar: when the active
    /// agent's most recently learned `agentCapabilities.loadSession` is
    /// `false` (this fixture's default), `:AiSessions` must refuse
    /// outright — no picker opens, and nothing else happens (the local
    /// index is never even consulted for its contents).
    #[cfg(unix)]
    #[test]
    fn acp_open_sessions_picker_refuses_when_agent_does_not_advertise_load_session() {
        let mut engine = engine_with_registered_fixture_agent(&[("ACP_FAKE_NO_TOOL_REQUEST", "1")]);

        engine.ai_send_message("hello".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert_eq!(
            engine.acp_session_index.load_session_capability("claude"),
            Some(false)
        );

        engine.acp_open_sessions_picker();

        assert!(
            !engine.picker_open,
            "the picker must not open for an agent that doesn't advertise \
             loadSession"
        );
        assert!(
            engine.message.contains("does not support"),
            "a message must explain the refusal: {:?}",
            engine.message
        );
    }

    /// Review blocking finding: resuming a *different* session while the
    /// current one is still live must replace the displayed transcript,
    /// not splice the resumed history onto whatever was already on
    /// screen. `Engine::acp_resume_session`'s own doc names this exact
    /// state ("a live session already: call `acp_begin_session`
    /// immediately") — every other resume test calls `:AiClear`
    /// immediately beforehand, which happens to also empty `ai_messages`
    /// as a side effect of killing the client, so none of them ever
    /// actually exercised it.
    ///
    /// RED verified: with `Engine::acp_reset_transcript_for_resume`'s call
    /// removed from `acp_resume_session`, this fails — the live session's
    /// own "hello from session A" turn (and its reply) survive in
    /// `ai_messages` alongside the resumed session's replayed history.
    #[cfg(unix)]
    #[test]
    fn acp_resume_session_replaces_a_still_live_transcript_not_splices_it() {
        let mut engine = engine_with_registered_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_LOAD_SESSION", "1"),
        ]);

        engine.ai_send_message("hello from session A".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(
            engine.acp_client.is_some() && engine.acp_session_id.is_some(),
            "the live session must still be connected going into the resume"
        );
        assert!(
            engine
                .ai_messages
                .iter()
                .any(|m| m.content.contains("hello from session A")),
            "sanity: the live session's own turn must be on screen before \
             resuming: {:?}",
            engine.ai_messages
        );

        // The picker's confirm action, with no intervening `:AiClear` —
        // the live client and its session stay connected until
        // `acp_resume_session` itself tears the turn down. This fixture's
        // `session/load` reply hardcodes "sess-1" on every replayed
        // `session/update` notification (see its top-of-file doc), which
        // happens to be the same id `session/new` assigned above — that's
        // fine and deliberate: what this test checks is that the resume
        // clears the transcript *before* the replay lands, not that the
        // resumed id differs from the one already live.
        engine.acp_resume_session("sess-1".to_string());
        poll_acp_until(&mut engine, |e| {
            e.ai_messages
                .iter()
                .any(|m| m.content.contains("It prints hello."))
        });

        assert_eq!(
            engine.acp_session_id.as_deref(),
            Some("sess-1"),
            "must resume the requested session id"
        );
        assert!(
            !engine
                .ai_messages
                .iter()
                .any(|m| m.content.contains("hello from session A")),
            "the previous live session's transcript must be gone, not \
             spliced in ahead of the resumed history: {:?}",
            engine.ai_messages
        );
        assert!(
            engine
                .ai_messages
                .iter()
                .any(|m| m.content.contains("what does main.rs do")),
            "the resumed session's replayed history must be the only \
             thing on screen: {:?}",
            engine.ai_messages
        );
    }

    /// Review blocking finding: the `acp_reopen_last_session` auto-resume
    /// path must not put the brand-new message *above* the "past"
    /// conversation it's meant to continue. `ai_send_message_via_acp` used
    /// to push the typed message onto `ai_messages` unconditionally before
    /// checking whether an auto-resume was about to happen, so the
    /// replayed `session/update` history (which lands after whatever is
    /// already in `ai_messages`) ended up sandwiched *between* the new
    /// message and its own reply.
    ///
    /// RED verified: reverting `ai_send_message_via_acp` to push
    /// `displayed_text` unconditionally before the `acp_reopen_last_session`
    /// check (and dropping the `acp_pending_prompt_display` deferral) makes
    /// this fail — "continuing the chat" (the new message) sorts before
    /// "what does main.rs do" (the replayed history) in `ai_messages`.
    #[cfg(unix)]
    #[test]
    fn acp_reopen_last_session_defers_the_new_message_after_the_replayed_history() {
        let mut engine = engine_with_registered_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_LOAD_SESSION", "1"),
        ]);
        engine.settings.acp_reopen_last_session = true;
        let cwd = engine.acp_workspace_cwd();
        engine
            .acp_session_index
            .record_session("sess-1", "claude", &cwd, "an older conversation");
        engine
            .acp_session_index
            .set_load_session_capability("claude", true);

        // The very first `:AI` message of the process — this is the one
        // and only chance `acp_reopen_last_session` gets to fire.
        engine.ai_send_message("continuing the chat".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);

        assert_eq!(
            engine.acp_session_id.as_deref(),
            Some("sess-1"),
            "must have resumed the recorded session, not started a fresh one"
        );

        let contents: Vec<String> = engine
            .ai_messages
            .iter()
            .map(|m| m.content.clone())
            .collect();
        let idx_replayed = contents
            .iter()
            .position(|c| c.contains("what does main.rs do"))
            .unwrap_or_else(|| panic!("replayed history must be present: {contents:?}"));
        let idx_new_message = contents
            .iter()
            .position(|c| c.contains("continuing the chat"))
            .unwrap_or_else(|| panic!("the new message must be present: {contents:?}"));
        let idx_new_reply = contents
            .iter()
            .position(|c| c.contains("Hello world"))
            .unwrap_or_else(|| panic!("the new reply must be present: {contents:?}"));

        assert!(
            idx_replayed < idx_new_message,
            "replayed history must come before the newly typed message: \
             {contents:?}"
        );
        assert!(
            idx_new_message < idx_new_reply,
            "the newly typed message must come before its own reply: \
             {contents:?}"
        );
    }

    /// Review blocking finding: a failed `session/load` must not leave
    /// `acp_session_id` pointing at a session id the live agent never
    /// actually created — the next prompt must fall back to a fresh
    /// session rather than silently talking to an id nothing on the other
    /// end recognises. `tests/fixtures/fake_acp_agent.sh`'s
    /// `$ACP_FAKE_LOAD_SESSION_ERROR` flag exists specifically for this
    /// regression path but, before this test, was never referenced by any
    /// Rust test.
    ///
    /// RED verified: with the `AcpEvent::RequestFailed` handler's
    /// `method == "session/load"` branch reverted to the generic case
    /// (no `acp_session_id` reset, no fallback `acp_begin_session` call),
    /// `engine.acp_session_id` stays `Some("sess-stale")` forever and the
    /// follow-up `ai_send_message` sends a bare `session/prompt` against
    /// it instead of falling back to a fresh session — the fixture doesn't
    /// recognise that id for `session/prompt` either, so no reply ever
    /// lands and this test's final assertion times out.
    #[cfg(unix)]
    #[test]
    fn failed_session_load_resets_session_id_and_falls_back_to_a_fresh_session() {
        let mut engine = engine_with_registered_fixture_agent(&[
            ("ACP_FAKE_NO_TOOL_REQUEST", "1"),
            ("ACP_FAKE_LOAD_SESSION", "1"),
            ("ACP_FAKE_LOAD_SESSION_ERROR", "1"),
        ]);
        let cwd = engine.acp_workspace_cwd();
        engine.acp_session_index.record_session(
            "sess-stale",
            "claude",
            &cwd,
            "a session the agent has forgotten",
        );
        engine
            .acp_session_index
            .set_load_session_capability("claude", true);

        engine.acp_resume_session("sess-stale".to_string());
        poll_acp_until(&mut engine, |e| e.message.contains("session/load failed"));

        assert_eq!(
            engine.acp_session_id, None,
            "a failed session/load must not leave acp_session_id pointing \
             at a session the agent never actually created"
        );

        // The fallback `session/new` this triggers is a separate round
        // trip — let it land before driving the panel further.
        poll_acp_until(&mut engine, |e| e.acp_session_id.is_some());
        assert_ne!(
            engine.acp_session_id.as_deref(),
            Some("sess-stale"),
            "the fallback session must not silently reuse the stale id"
        );

        // The panel must still be usable afterwards.
        engine.ai_send_message("hello after the failed resume".to_string());
        poll_acp_until(&mut engine, |e| !e.ai_streaming);
        assert!(
            engine
                .ai_messages
                .iter()
                .any(|m| m.content.contains("Hello world")),
            "the fallback session must actually work: {:?}",
            engine.ai_messages
        );
    }
}
