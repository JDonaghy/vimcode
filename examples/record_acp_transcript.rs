//! Dev tool: record a fresh ACP transcript for the `tests/fixtures/
//! acp_transcripts/` corpus (#1461).
//!
//! Drives a fixed, generic session — `initialize` -> `session/new` ->
//! `session/prompt "hi"` -> drain events until `PromptStopped`/agent exit,
//! auto-answering any agent -> client request with an empty `{}` result —
//! against whatever agent command is given on the command line, and writes
//! the two-directional wire transcript to the given output path via
//! [`vimcode_core::core::acp::AcpClient::spawn_with_recording`].
//!
//! ## Re-recording against the real `claude-agent-acp` adapter
//!
//! This repo's CI (and this sandbox) has no Node.js and no authenticated
//! agent login (#951), so the corpus actually committed under
//! `tests/fixtures/acp_transcripts/` was generated against the checked-in
//! `tests/fixtures/fake_acp_agent.sh` fixture, in scripted branches
//! (`ACP_FAKE_TOOL_CALL`, `ACP_FAKE_NO_TOOL_REQUEST`) chosen to match the
//! wire shapes the real adapter is documented (in `core::acp`'s module doc
//! and the #1454/#1444 postmortems) to actually send — fragment
//! `oldText`/`newText` diffs, `type: "terminal"` auth methods carrying
//! `args`, etc. — not literally captured from a live session.
//!
//! To re-record against a *real* adapter once one is available (e.g. on a
//! machine with Node.js and an authenticated `claude-agent-acp` login),
//! pin the adapter's version first, then:
//!
//! ```text
//! cargo run --example record_acp_transcript -- \
//!   tests/fixtures/acp_transcripts/OUTPUT_NAME.transcript -- \
//!   npx @agentclientprotocol/claude-agent-acp
//! ```
//!
//! Everything after the bare `--` is the agent's own argv (its first word
//! is the program). Substitute any absolute path the real session touches
//! with the `__FIXTURE_DIR__` placeholder token before committing — see
//! `tests/acp_contract.rs`'s doc comment for why (`AcpClient` resolves
//! wire-reported paths against the workspace root, and a contract test
//! substitutes a fresh temp directory for that token per run so recordings
//! never race each other or bake in one machine's `/tmp` layout).
//!
//! A scenario needing a specific scripted reply to an agent -> client
//! request (e.g. `session/request_permission` choosing something other
//! than "allow the default") needs this file's auto-answer loop edited
//! before running — it is a generic recorder, not a scenario-specific one.

use vimcode_core::core::acp::{AcpClient, AcpEvent};

fn main() {
    let mut args = std::env::args().skip(1);
    let output = args
        .next()
        .unwrap_or_else(|| usage("missing <output-path>"));
    let sep = args.next();
    if sep.as_deref() != Some("--") {
        usage("expected `--` before the agent command");
    }
    let argv: Vec<String> = args.collect();
    if argv.is_empty() {
        usage("missing agent command after `--`");
    }

    let cwd = std::env::temp_dir();
    let mut client =
        AcpClient::spawn_with_recording(&argv, &cwd, &[], Some(std::path::Path::new(&output)))
            .unwrap_or_else(|e| {
                eprintln!("record_acp_transcript: failed to spawn agent: {e}");
                std::process::exit(1);
            });

    client.initialize();
    let mut prompted = false;
    loop {
        for event in client.poll() {
            match event {
                AcpEvent::Initialized { .. } => {
                    client.new_session(&cwd, vec![]);
                }
                AcpEvent::SessionCreated { session_id, .. } if !prompted => {
                    prompted = true;
                    client.prompt(
                        &session_id,
                        vec![serde_json::json!({"type": "text", "text": "hi"})],
                    );
                }
                AcpEvent::ClientRequest { request_id, .. } => {
                    client.respond_to_client_request(request_id, Ok(serde_json::json!({})));
                }
                AcpEvent::PromptStopped { .. } | AcpEvent::AgentExited { .. } => {
                    drop(client);
                    println!("record_acp_transcript: wrote {output}");
                    return;
                }
                _ => {}
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn usage(msg: &str) -> ! {
    eprintln!("record_acp_transcript: {msg}");
    eprintln!("usage: record_acp_transcript <output-path> -- <agent-argv...>");
    std::process::exit(1);
}
