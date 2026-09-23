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
}
