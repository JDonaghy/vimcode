//! `Engine::poll_acp` — the one call site `poll_idle` uses to drain the ACP
//! transport (#951 ACP-0 built the transport; #952 ACP-1 adds the session
//! lifecycle drive and the `session/update` -> AI-panel chunk mapping this
//! module implements). `tool_call`/`tool_call_update`/`plan` updates and
//! agent -> client requests (`fs/*`, `session/request_permission`) are
//! intentionally left unhandled here — parked/ignored without breaking the
//! stream — per ACP-1's scope; later ACP slices (ACP-2 fs bridge, ACP-4/5
//! tool-call + plan rendering) add real handling on top of the same
//! `AcpEvent` stream, not a transport change.

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
                    self.acp_client = None;
                    self.acp_session_id = None;
                    self.acp_pending_prompt = None;
                    self.acp_streaming_turn = None;
                    self.ai_streaming = false;
                    redraw = true;
                }
                AcpEvent::Initialized { .. } => {
                    // Handshake step 2: now that the agent answered
                    // `initialize`, open a session. `ai_send_message`
                    // already queued the user's prompt in
                    // `acp_pending_prompt`, sent once `SessionCreated`
                    // lands below.
                    let cwd = self.acp_workspace_cwd();
                    if let Some(client) = self.acp_client.as_mut() {
                        client.new_session(&cwd, vec![]);
                    }
                    redraw = true;
                }
                AcpEvent::SessionCreated { session_id, .. } => {
                    self.acp_session_id = Some(session_id.clone());
                    if let Some(text) = self.acp_pending_prompt.take() {
                        if let Some(client) = self.acp_client.as_mut() {
                            client.prompt(
                                &session_id,
                                vec![serde_json::json!({"type": "text", "text": text})],
                            );
                        }
                    } else {
                        // No prompt was waiting on this handshake — nothing
                        // to stream, so the panel shouldn't sit "thinking".
                        self.ai_streaming = false;
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
                        if let Some((kind, text)) =
                            inner.and_then(crate::core::acp::session_update_chunk)
                        {
                            self.acp_append_chunk(kind, text);
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
                    if method == "session/prompt" {
                        self.ai_streaming = false;
                        self.acp_streaming_turn = None;
                    }
                    self.message = format!("ACP {method} failed: {message}");
                    self.ai_messages.push(AiMessage {
                        role: "assistant-thought".to_string(),
                        content: format!("\u{26a0} {method} failed: {message}"),
                    });
                    redraw = true;
                }
                // Agent -> client requests (`fs/read_text_file`,
                // `session/request_permission`, ...): left parked, not
                // answered — the fs/* bridge and permission UI are later
                // ACP slices. Not answering doesn't break the transport
                // (`AcpClient::poll` keeps draining), it just means a turn
                // that needs one will not reach `PromptStopped` yet.
                AcpEvent::ClientRequest { .. } => {
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
    fn acp_workspace_cwd(&self) -> std::path::PathBuf {
        self.workspace_root
            .clone()
            .unwrap_or_else(|| self.cwd.clone())
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
}

#[cfg(test)]
mod tests {
    use crate::core::engine::Engine;

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
}
