//! `Engine::poll_acp` — the one call site `poll_idle` uses to drain the ACP
//! transport (#951, ACP-0). This slice is transport + session lifecycle
//! only: **no UI**. Most event variants are just forwarded to `redraw`
//! today; later ACP slices (session state tracking, the AI panel) will add
//! real handling here without touching the transport in `src/core/acp.rs`
//! or either backend.

use super::*;
use crate::core::acp::AcpEvent;

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
                    }
                    self.acp_client = None;
                    redraw = true;
                }
                // Session lifecycle and content events: nothing in this
                // slice renders them (no session state field exists yet).
                // Marking a redraw keeps the contract simple for whichever
                // later slice adds that state — it costs nothing when
                // nothing changed visibly.
                AcpEvent::Initialized { .. }
                | AcpEvent::SessionCreated { .. }
                | AcpEvent::PromptStopped { .. }
                | AcpEvent::SessionUpdate { .. }
                | AcpEvent::ClientRequest { .. }
                | AcpEvent::RequestFailed { .. } => {
                    redraw = true;
                }
            }
        }
        redraw
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

    #[cfg(unix)]
    #[test]
    fn poll_acp_drains_agent_exit_into_the_status_message_and_clears_the_client() {
        // Full transport-level lifecycle coverage (initialize -> session/new
        // -> session/prompt -> stopReason: end_turn, and the bidirectional
        // dispatch/parking acceptance criteria) lives in
        // `src/core/acp.rs`'s own tests against the fake agent fixture —
        // that's the transport's contract, not the engine's. What the
        // engine adds on top is exactly one thing: draining `AgentExited`
        // clears `acp_client` and surfaces a message. Cover that here with
        // the real fixture rather than re-deriving the whole lifecycle.
        let argv = vec![
            "sh".to_string(),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_acp_agent.sh"
            )
            .to_string(),
        ];
        let cwd = std::env::temp_dir();
        let mut client = crate::core::acp::AcpClient::spawn_with_env(
            &argv,
            &cwd,
            &[("ACP_FAKE_DIE_AFTER_INIT", "1")],
        )
        .expect("fixture agent should spawn");
        client.initialize();

        let mut engine = Engine::new_for_test();
        engine.acp_client = Some(client);

        // First poll: Initialized event only, client stays alive.
        let start = std::time::Instant::now();
        loop {
            if engine.poll_acp() || start.elapsed() > std::time::Duration::from_secs(5) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            engine.acp_client.is_some(),
            "still running after Initialized"
        );

        // Second poll: AgentExited — engine clears the client and reports it.
        let start = std::time::Instant::now();
        loop {
            if engine.acp_client.is_none() || start.elapsed() > std::time::Duration::from_secs(5) {
                break;
            }
            engine.poll_acp();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            engine.acp_client.is_none(),
            "agent exit should clear the client"
        );
        assert_eq!(engine.message, "ACP agent exited");
    }
}
