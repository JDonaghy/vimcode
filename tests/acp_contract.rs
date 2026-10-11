//! ACP contract tests: drive a real `Engine` against a *recorded*
//! NDJSON transcript replayed by the `acp-replay-agent` binary (`tests/
//! fixtures/replay_acp_agent.rs`), through the exact same transport code
//! path (`AcpClient::spawn_with_env` -> `poll_acp` -> `AcpEvent` dispatch)
//! a real session uses — not a shortcut that calls an engine function
//! directly and skips NDJSON parsing entirely, and not the hand-scripted
//! `fake_acp_agent.sh` fixture (which only ever emits the shapes the
//! client-side code already expects; a corpus recorded ahead of time and
//! replayed byte-for-byte is a stronger, independent check on the wire
//! contract — see `core::acp`'s `spawn_with_recording` doc and `examples/
//! record_acp_transcript.rs` for how the corpus under `tests/fixtures/
//! acp_transcripts/` was produced).
//!
//! ## Corpus provenance
//!
//! CI (and the sandbox this was authored in) has no Node.js and no
//! authenticated `claude-agent-acp` login (#951's standing constraint), so
//! every transcript here was recorded against the checked-in `fake_acp_
//! agent.sh` fixture in the scripted branch that matches what the real
//! adapter is documented to send (`ACP_FAKE_TOOL_CALL`'s fragment `oldText`/
//! `newText`, per `core::acp`'s module doc and the #1454 postmortem) —
//! **not** literally captured from a live session. `examples/
//! record_acp_transcript.rs` documents how to re-record against a real
//! adapter once one is reachable.
//!
//! One exception: `turn_write_hunk_review.transcript` is
//! *hand-authored* to the exact shape `fake_acp_agent.sh`'s
//! `ACP_FAKE_FS_WRITE_PATH` branch emits, because `record_acp_transcript`
//! auto-answers every agent -> client request with `{}` and therefore never
//! actually serves the `fs/write_text_file` that scenario is about. Same
//! class of provenance as the rest of the corpus (scripted-fixture shape,
//! not a live capture) — see `docs/ACP_CONTRACT_TESTS.md`.
//!
//! ## The `__FIXTURE_DIR__` placeholder
//!
//! A transcript's `diff`/`tool_call` `path` fields point at a placeholder
//! token, `__FIXTURE_DIR__`, substituted with a fresh per-test temp
//! directory before replay (see [`instantiate_transcript`]) — so replaying
//! the same committed corpus file never collides across parallel test runs
//! or bakes in the recording machine's own `/tmp` layout into a committed
//! fixture.
//!
//! ## What replay validates (and what it doesn't)
//!
//! `acp-replay-agent` checks that the live client's next message matches
//! the transcript's next expected message **by JSON-RPC method name only**
//! — not deep parameter equality (see that binary's own module doc). A
//! contract test therefore does not need to reproduce the exact prompt text
//! the transcript was originally recorded with; what it proves is that a
//! real client, driven by `Engine`'s real ACP dispatch, produces the same
//! *sequence of message kinds* a real session did, and that the engine
//! reacts correctly to the exact wire *shapes* the agent sent back.

use std::path::{Path, PathBuf};
use vimcode_core::core::acp::AcpClient;
use vimcode_core::core::engine::Engine;

/// Generous for the same reason every other ACP fixture test in this repo
/// is (see `core::acp::tests::TEST_DEADLINE`'s doc): a `cargo test` run
/// driving thousands of tests concurrently can deschedule a subprocess
/// fork+exec + pipe round-trip far longer than it costs standalone, and
/// every wait loop below exits the instant its condition holds, so a
/// larger bound only matters on the failing path.
const TEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

fn corpus_path(name: &str) -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/acp_transcripts"
    ))
    .join(name)
}

/// Read corpus transcript `name`, substitute every `__FIXTURE_DIR__`
/// placeholder with `fixture_dir`, and write the result to a fresh temp
/// file unique to this process+thread — the instantiated copy
/// `acp-replay-agent` actually reads.
fn instantiate_transcript(name: &str, fixture_dir: &Path) -> PathBuf {
    let template = std::fs::read_to_string(corpus_path(name))
        .unwrap_or_else(|e| panic!("corpus transcript {name} should exist and be readable: {e}"));
    let instantiated = template.replace("__FIXTURE_DIR__", &fixture_dir.to_string_lossy());
    let out = std::env::temp_dir().join(format!(
        "vimcode-acp-contract-{name}-{}-{:?}.transcript",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&out, instantiated)
        .unwrap_or_else(|e| panic!("failed to write instantiated transcript: {e}"));
    out
}

/// Spawn a real `AcpClient` whose "agent" is `acp-replay-agent` playing
/// back `transcript_name` (with `__FIXTURE_DIR__` resolved to
/// `fixture_dir`), `initialize()` it, and wire it onto a fresh `Engine` —
/// the exact same "already-spawned-above" pattern
/// `core::engine::acp_ops::tests::engine_with_fixture_agent` uses against
/// the live shell fixture, here pointed at the replay binary instead.
fn engine_with_replay(transcript_name: &str, fixture_dir: &Path) -> Engine {
    let transcript = instantiate_transcript(transcript_name, fixture_dir);
    let argv = vec![
        env!("CARGO_BIN_EXE_acp-replay-agent").to_string(),
        transcript.to_string_lossy().into_owned(),
    ];
    let cwd = std::env::temp_dir();
    let mut client =
        AcpClient::spawn_with_env(&argv, &cwd, &[]).expect("replay agent should spawn");
    client.initialize(true);

    // `Engine::new_for_test` is `#[cfg(test)]`-only and therefore invisible
    // from an external integration-test crate (only compiled when
    // `vimcode_core` itself is built as a test target) — use the same
    // "suppress disk I/O, then override settings" pattern `tests/
    // ai_panel.rs`'s `engine()` helper does instead.
    vimcode_core::core::session::suppress_disk_saves();
    vimcode_core::core::session::suppress_disk_loads();
    let mut engine = Engine::new();
    engine.settings = vimcode_core::core::settings::Settings::default();
    engine.acp_mut().client = Some(client);
    // Routes `ai_send_message` onto the already-spawned client above rather
    // than trying to spawn a new one from this (nonexistent) command line.
    engine.settings.acp_agent_command = "already-spawned-above".to_string();
    engine
}

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

fn unique_temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vimcode-acp-contract-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Baseline contract: `basic_session.transcript` (initialize -> session/new
/// -> session/prompt -> two streamed `agent_message_chunk`s -> stopReason:
/// end_turn) replayed through a real `AcpClient`/`Engine::poll_acp` must
/// merge the streamed chunks into one assistant turn and clear
/// `ai_streaming` — proving the record/replay round trip preserves the
/// exact wire shape a fresh recording would produce (this transcript is
/// asserted byte-for-byte in `core::acp::tests::
/// spawn_with_recording_captures_both_directions_of_the_wire`, which
/// records the same scenario fresh every run).
#[test]
fn basic_session_transcript_streams_and_completes_via_replay() {
    let dir = unique_temp_dir("basic");
    let mut engine = engine_with_replay("basic_session.transcript", &dir);

    engine.ai_send_message("hello".to_string());
    assert!(
        engine.acp().ai_streaming,
        "sending a message must mark it busy"
    );

    poll_acp_until(&mut engine, |e| !e.acp().ai_streaming);
    assert!(
        !engine.acp().ai_streaming,
        "the turn should reach stopReason: end_turn within {TEST_DEADLINE:?}"
    );
    assert_eq!(
        engine.acp().ai_messages.last().map(|m| m.content.as_str()),
        Some("Hello world"),
        "the two streamed agent_message_chunk notifications must merge into \
         one assistant turn: {:?}",
        engine.acp().ai_messages
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Display-only contract test: a `diff` content block does not write to
/// disk and does not open a review surface. The `diff` block is for display
/// only per the ACP spec. The `fs/write_text_file` and its review are tested
/// separately in [`turn_write_transcript_opens_a_hunk_level_turn_review_over_the_wire`].
#[test]
fn fragment_diff_transcript_is_display_only_and_never_writes() {
    let dir = unique_temp_dir("fragment");
    let target = dir.join("target.txt");
    std::fs::write(&target, "line1\nold line\nline3\n").unwrap();

    let mut engine = engine_with_replay("fragment_diff_edit.transcript", &dir);
    engine.workspace_root = Some(dir.clone());

    engine.ai_send_message("please edit".to_string());
    poll_acp_until(&mut engine, |e| !e.acp().ai_streaming);
    assert!(
        !engine.acp().ai_streaming,
        "the turn should reach stopReason: end_turn within {TEST_DEADLINE:?}"
    );

    assert!(
        engine.change_review.is_none(),
        "a diff content block is display-only — it must never open the \
         change-review surface"
    );
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "line1\nold line\nline3\n",
        "a turn whose only edit report was a diff block must leave the file \
         byte-identical on disk"
    );
    // The "annotate the tool card" half of the design is untouched: the
    // block is still stored on the call, so the card still renders its
    // `+1 -1 target.txt` summary.
    let calls = &engine.acp().tool_calls;
    assert_eq!(calls.len(), 1, "the turn's one tool call must be tracked");
    assert_eq!(
        calls[0].content.len(),
        1,
        "the diff block must still be stored on the call for display"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Hunk-level Keep/Reject over the real transport: `turn_write_hunk_review.
/// transcript` contains a `tool_call`, an `fs/write_text_file` request with
/// the whole new file body, and a `tool_call_update` with the same edit as a
/// `diff` block. The write lands exactly once; the review opens at the turn
/// level over what was really written, not from the diff block; and `r`
/// reverts only the hunk under the cursor, leaving other hunks on disk.
#[test]
fn turn_write_transcript_opens_a_hunk_level_turn_review_over_the_wire() {
    let dir = unique_temp_dir("turn-write-hunks");
    let target = dir.join("target.txt");
    let pre_turn: String = (1..=20)
        .map(|n| format!("{n}\n"))
        .collect::<Vec<_>>()
        .concat();
    std::fs::write(&target, &pre_turn).unwrap();

    let mut engine = engine_with_replay("turn_write_hunk_review.transcript", &dir);
    engine.workspace_root = Some(dir.clone());
    // The `auto` mode opens a modal review when a turn ends.
    engine.settings.acp_review_on_turn_end = vimcode_core::core::settings::AcpReviewOnTurnEnd::Auto;

    engine.ai_send_message("please edit two places".to_string());
    poll_acp_until(&mut engine, |e| !e.acp().ai_streaming);
    assert!(
        !engine.acp().ai_streaming,
        "the turn should reach stopReason: end_turn within {TEST_DEADLINE:?}"
    );

    // The agent's real `fs/write_text_file` landed — exactly once, not
    // double-applied by the diff block that reported the same edit.
    let after_turn = std::fs::read_to_string(&target).unwrap();
    assert!(
        after_turn.contains("\nAGENT-A\n") && after_turn.contains("\nAGENT-B\n"),
        "fs/write_text_file must have applied the agent's whole new body: \
         {after_turn:?}"
    );

    let review = engine
        .change_review
        .as_ref()
        .expect("ending a turn that wrote a file must open the turn review");
    assert_eq!(
        review.entries.len(),
        1,
        "exactly one entry — the one file the turn wrote — not a second \
         proposal entry from the diff block"
    );
    assert!(
        engine.turn_review_checkpoint_id.is_some(),
        "the opened surface must be a turn review (checkpoint-backed), the \
         only kind whose a/r act per hunk"
    );
    let entry = review.current_entry().expect("a current entry");
    assert_eq!(
        entry.change.old_text.as_deref(),
        Some(pre_turn.as_str()),
        "old_text must be the file's pre-turn content, whole"
    );
    assert_eq!(
        entry.change.new_text, after_turn,
        "new_text must be what the agent actually wrote"
    );
    assert!(
        entry.view.hunks.len() >= 2,
        "the two well-separated edits must land in separate hunks, or there \
         is nothing per-hunk to decide: {} hunk(s)",
        entry.view.hunks.len()
    );

    // `r` on the first hunk reverts ONLY that hunk.
    engine.handle_change_review_key("", Some('r'));
    let after_reject = std::fs::read_to_string(&target).unwrap();
    assert!(
        !after_reject.contains("AGENT-A"),
        "the rejected hunk's agent content must be gone: {after_reject:?}"
    );
    assert!(
        after_reject.contains("\n3\n"),
        "the rejected hunk must be back to its pre-turn line \"3\": \
         {after_reject:?}"
    );
    assert!(
        after_reject.contains("\nAGENT-B\n"),
        "the OTHER hunk's agent content must survive a hunk-level reject: \
         {after_reject:?}"
    );
    assert!(
        engine.change_review.is_some(),
        "the second hunk is still undecided — the surface must stay open"
    );

    // Keeping the remaining hunk decides the entry and auto-closes.
    engine.handle_change_review_key("", Some('a'));
    assert!(
        engine.change_review.is_none(),
        "every hunk now has a decision — the surface must auto-close"
    );
    let final_content = std::fs::read_to_string(&target).unwrap();
    assert!(
        final_content.contains("\nAGENT-B\n") && !final_content.contains("AGENT-A"),
        "a mixed keep/reject result must survive on disk: {final_content:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
