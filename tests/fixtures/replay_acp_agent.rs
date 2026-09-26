//! ACP transcript replay agent (#1461) — plays the *agent* side of a
//! recorded ACP session back at a real `AcpClient`, so contract tests can
//! drive the engine's actual NDJSON transport + dispatch path against a
//! fixed, realistic wire recording instead of either (a) a hand-scripted
//! fixture that only ever emits what the client-side code already expects,
//! or (b) calling engine functions directly, skipping the transport layer
//! entirely.
//!
//! ## Transcript format
//!
//! A transcript is exactly what [`AcpClient::spawn_with_recording`]
//! (`src/core/acp.rs`) writes: one line per NDJSON message, in the order it
//! crossed the wire, prefixed `"> "` for client->agent or `"< "` for
//! agent->client. This binary reads that same format back — i.e. a
//! transcript recorded from *this repo's own* `fake_acp_agent.sh` fixture
//! (or, with a real adapter's Node binary and login on a machine that has
//! them, from an actual `claude-agent-acp` session — CI has neither, see
//! `fake_acp_agent.sh`'s own header) replays identically either way. The
//! transcript's `"> "` lines are never re-parsed for content beyond
//! `method`/`id` (see below) — they exist so this program knows how many
//! client messages to consume between each `"< "` line it emits, and so it
//! can fail loudly on a mismatch rather than silently drifting out of sync.
//!
//! ## Why replay needs *any* JSON logic (unlike `fake_acp_agent.sh`)
//!
//! `fake_acp_agent.sh` gets away with zero JSON parsing because it always
//! answers with a fixed, hand-picked id per branch. A transcript recorded
//! from a real session bakes in whatever ids *that* session's client
//! happened to assign — which will not, in general, match the ids a fresh
//! `AcpClient` assigns when replaying the same script (e.g. a session that
//! re-initializes for terminal auth assigns id 1 twice, once per
//! `initialize` call). So for every recorded `"> "` line that is a
//! client-initiated *request* (has both `method` and `id`), this program
//! remembers `recorded_id -> live_id` (the id actually read off the live
//! client's matching request); every subsequent recorded `"< "` line that
//! is a *response* (has `id` but no `method`) has its `id` rewritten
//! through that map before being sent. A recorded `"< "` line that has its
//! own `method` (an agent -> client request, e.g.
//! `session/request_permission`) is an id the agent itself picked and is
//! sent unmodified — the live client will echo it back verbatim in its
//! reply, so there is nothing to remap.
//!
//! ## Usage
//!
//! ```text
//! acp-replay-agent <transcript-path>
//! ```
//!
//! Reads NDJSON on stdin, writes NDJSON on stdout — the same shape
//! `AcpClient::spawn`/`spawn_with_env` expects from any agent subprocess.
//! A mismatch between what the live client actually sent and what the
//! transcript expected next is a loud failure: a diagnostic line to stderr
//! and a non-zero exit, never a silent skip — a contract test whose replay
//! agent quietly went along with an unexpected request would stop proving
//! anything about the recorded scenario.

use serde_json::Value;
use std::io::{BufRead, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    ToAgent,
    ToClient,
}

struct Step {
    dir: Dir,
    raw: String,
    json: Value,
}

fn load_transcript(path: &str) -> Vec<Step> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| fail(&format!("cannot read transcript {path:?}: {e}")));
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (dir, raw) = if let Some(rest) = line.strip_prefix("> ") {
            (Dir::ToAgent, rest)
        } else if let Some(rest) = line.strip_prefix("< ") {
            (Dir::ToClient, rest)
        } else {
            fail(&format!(
                "transcript line {} has no '> '/'< ' direction prefix: {line:?}",
                n + 1
            ));
        };
        let json: Value = serde_json::from_str(raw).unwrap_or_else(|e| {
            fail(&format!(
                "transcript line {} is not valid JSON: {e}: {raw:?}",
                n + 1
            ))
        });
        steps.push(Step {
            dir,
            raw: raw.to_string(),
            json,
        });
    }
    steps
}

fn fail(msg: &str) -> ! {
    eprintln!("acp-replay-agent: {msg}");
    std::process::exit(1);
}

/// `id` as recorded/observed — ACP ids are JSON-RPC ids, always numbers on
/// this client's own wire (see `AcpClient::send_request`), but read
/// defensively since a malformed transcript should fail loudly here, not
/// panic.
fn json_id(v: &Value) -> Option<i64> {
    v.get("id").and_then(|id| id.as_i64())
}

fn json_method(v: &Value) -> Option<&str> {
    v.get("method").and_then(|m| m.as_str())
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| fail("usage: acp-replay-agent <transcript-path>"));
    let steps = load_transcript(&path);

    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // recorded client-request id -> id the live client actually used.
    let mut id_map: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();

    for step in &steps {
        match step.dir {
            Dir::ToAgent => {
                let line = match lines.next() {
                    Some(Ok(l)) => l,
                    Some(Err(e)) => fail(&format!("error reading from client: {e}")),
                    None => fail(&format!(
                        "client closed stdin early; still expected: {}",
                        step.raw
                    )),
                };
                let live: Value = serde_json::from_str(&line)
                    .unwrap_or_else(|e| fail(&format!("client sent non-JSON line: {e}: {line:?}")));

                if let Some(expected_method) = json_method(&step.json) {
                    let got_method = json_method(&live);
                    if got_method != Some(expected_method) {
                        fail(&format!(
                            "expected next client message to be method {expected_method:?}, got {got_method:?}: {line}"
                        ));
                    }
                    if let (Some(recorded_id), Some(live_id)) =
                        (json_id(&step.json), json_id(&live))
                    {
                        id_map.insert(recorded_id, live_id);
                    }
                }
                // A recorded response (no `method`) — the client answering
                // one of *our* agent-initiated requests. Its `id` is
                // whatever we sent unmodified, so there is nothing to
                // remap; the content isn't otherwise inspected (the corpus
                // is scripted, not echoing).
            }
            Dir::ToClient => {
                let mut msg = step.json.clone();
                let is_response = json_method(&msg).is_none() && msg.get("id").is_some();
                if is_response {
                    if let Some(recorded_id) = json_id(&msg) {
                        if let Some(live_id) = id_map.get(&recorded_id) {
                            msg["id"] = serde_json::json!(*live_id);
                        } else {
                            fail(&format!(
                                "no live id recorded for response id {recorded_id}: {}",
                                step.raw
                            ));
                        }
                    }
                }
                let encoded = serde_json::to_string(&msg)
                    .unwrap_or_else(|e| fail(&format!("failed to re-encode transcript line: {e}")));
                if writeln!(out, "{encoded}").is_err() || out.flush().is_err() {
                    fail("client closed stdout; cannot continue replay");
                }
            }
        }
    }
}
