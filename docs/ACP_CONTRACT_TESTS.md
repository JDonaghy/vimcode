# ACP contract tests: recording, replaying, and the corpus (#1461)

## Why this exists

Every ACP test before #1461 drove `Engine`/`AcpClient` against
`tests/fixtures/fake_acp_agent.sh` — a hand-written fixture that only ever
emits the wire shapes vimcode's own code already expected. Three real bugs
(#1443, #1444, #1454) shipped past every one of those tests because the fake
agent didn't reproduce the real adapter's behavior in each case: #1454 in
particular was the fake sending whole-file diffs when a real adapter
(`@agentclientprotocol/claude-agent-acp`) sends only the edited *fragment*
(`oldText`/`newText` covering the changed region, not the whole file) — data
loss when accepted.

This is infrastructure to close that gap: **record** a real wire session,
**replay** it deterministically against a real `AcpClient`, and write
**contract tests** that assert on user-visible outcomes (file contents,
transcript, dialogs) driven through the actual transport, not a shortcut
that calls an engine function directly.

## The three pieces

| Piece | Where |
|---|---|
| Record mode | `AcpClient::spawn_with_recording` (`src/core/acp.rs`) |
| Replay agent | `tests/fixtures/replay_acp_agent.rs` (built as the `acp-replay-agent` bin) |
| Contract tests | `tests/acp_contract.rs` |
| Corpus | `tests/fixtures/acp_transcripts/*.transcript` |
| Recording tool | `examples/record_acp_transcript.rs` |

### Record mode

`AcpClient::spawn_with_recording(argv, cwd, extra_env, Some(record_to))`
tees every raw NDJSON line, in both directions, into `record_to` as it
crosses the wire — one line per message, prefixed `"> "` (client -> agent)
or `"< "` (agent -> client), byte-identical to what was actually sent (no
re-encoding). `AcpClient::spawn`/`spawn_with_env` are unchanged thin
wrappers (`record_to: None`).

### Transcript format

```
> {"id":1,"jsonrpc":"2.0","method":"initialize","params":{...}}
< {"jsonrpc":"2.0","id":1,"result":{...}}
> {"id":2,"jsonrpc":"2.0","method":"session/new","params":{...}}
< {"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-1"}}
...
```

Any path a scenario touches on disk is written in the corpus as the literal
placeholder token `__FIXTURE_DIR__` (e.g.
`"__FIXTURE_DIR__/target.txt"`) rather than the recording machine's real
temp path — `tests/acp_contract.rs` substitutes a fresh per-test temp
directory for that token before replay, so replaying a committed corpus
file never collides across parallel test runs or bakes in one machine's
`/tmp` layout.

### Replay agent (`acp-replay-agent`)

Plays the *agent* role: reads a transcript path as `argv[1]`, then for each
recorded step either (a) reads one line from real stdin and checks its
`method` matches the transcript's next expected `"> "` line — a mismatch is
a loud failure (stderr + non-zero exit), never a silent skip — or (b) writes
the next recorded `"< "` line to real stdout.

The one piece of real JSON logic it needs (and the reason it's a small Rust
binary, not another jq/python/node-free `/bin/sh` fixture like
`fake_acp_agent.sh`): a transcript's recorded request `id`s were assigned by
*whatever client recorded it*, and a fresh replay's live client will assign
its own, generally different, ids for the same requests (e.g. an
`initialize`/`session/new` pair repeats with the same starting id after a
terminal-auth re-init). So the replay agent remembers `recorded_id ->
live_id` for every client-initiated request it reads, and rewrites the `id`
field of every recorded *response* it sends accordingly. A recorded
agent-initiated request (e.g. `session/request_permission`) is sent with its
own id unmodified — the live client echoes it back verbatim, so there is
nothing to remap on that side.

**Important limitation:** the replay agent matches an incoming client
message by JSON-RPC `method` only, not deep parameter equality. A contract
test does not need to reproduce the exact prompt text (or any other
parameter) the transcript was originally recorded with — it only needs to
send the same *sequence of message kinds*. This keeps a corpus reusable
across contract tests that don't care about exact wording, at the cost of
not proving the client sent byte-identical parameters. If a future contract
test needs that stronger guarantee for a specific scenario, extend the
replay agent's matching for that step rather than assuming it's already
enforced everywhere.

### Corpus provenance — read this before trusting a transcript's realism

CI (and the environment this was authored in) has no Node.js and no
authenticated `claude-agent-acp` login (see `fake_acp_agent.sh`'s own
header, #951's standing constraint: "no ACP slice may depend on a real
adapter"). Every transcript currently committed under
`tests/fixtures/acp_transcripts/` was therefore recorded against
`fake_acp_agent.sh` itself, in the scripted branch (`ACP_FAKE_TOOL_CALL`,
`ACP_FAKE_NO_TOOL_REQUEST`) chosen to match the wire shape the real adapter
is *documented* to send (`core::acp`'s module doc, and the #1454
postmortem) — **not literally captured from a live session against the
real adapter.** Say so explicitly in any PR or status report that touches
this corpus; do not describe it as "recorded from claude-agent-acp" without
that caveat.

### Re-recording (including against a real adapter)

```bash
cargo run --example record_acp_transcript -- \
  tests/fixtures/acp_transcripts/OUTPUT_NAME.transcript -- \
  <agent argv...>
```

Everything after the bare `--` is the agent's own command line (first word
is the program). The tool drives a fixed, generic session — `initialize` ->
`session/new` -> `session/prompt "hi"` -> drain until `PromptStopped`/agent
exit, auto-answering any agent -> client request with `{}` — and records it
via `AcpClient::spawn_with_recording`. A scenario that needs a specific
scripted reply to an agent -> client request (e.g. choosing something other
than the default when a permission dialog is offered) needs the tool's
auto-answer loop edited before running; it is generic, not
scenario-specific.

To re-record against the fake fixture with a specific scripted branch, set
the fixture's env vars on the recorder's own process — they're inherited by
the spawned child:

```bash
ACP_FAKE_TOOL_CALL=1 ACP_FAKE_TOOL_CALL_PATH=__FIXTURE_DIR__/target.txt \
  cargo run --example record_acp_transcript -- \
  tests/fixtures/acp_transcripts/fragment_diff_edit.transcript -- \
  sh tests/fixtures/fake_acp_agent.sh
```

To re-record against a real adapter once one is reachable (a machine with
Node.js and an authenticated login), pin the adapter's version first (write
it into the corpus file's own commit message), then point the same command
at it, e.g. `npx @agentclientprotocol/claude-agent-acp`. Remember to
substitute any real absolute path the session touches with
`__FIXTURE_DIR__` before committing.

### Contract tests (`tests/acp_contract.rs`)

Each test: instantiate a corpus transcript against a fresh temp directory,
spawn a real `AcpClient` whose agent is `acp-replay-agent` (located via
`env!("CARGO_BIN_EXE_acp-replay-agent")`), wire it onto a real `Engine` the
same "already-spawned-above" way `core::engine::acp_ops`'s own fixture-driven
tests do, drive a real turn (`Engine::ai_send_message` + `poll_acp`), and
assert on a user-visible outcome — not on state merely being populated (see
`CLAUDE.md`'s "Rendered output, not state" rule, which applies here too:
`change_review.is_some()` alone doesn't prove the *content* is right,
so the test asserts the entry's actual `old_text`/`new_text`, and the
actual bytes on disk after accepting).

`fragment_diff_transcript_preserves_surrounding_file_content_on_accept` is
the issue's own acceptance bar ("reverting the #1454 fix makes a contract
test fail") — verified RED against the pre-#1454 pass-through during this
work (temporarily reverting `Engine::acp_open_review_for_diffs` to feed the
wire fragment straight through, confirming the test failed on both the
opened entry's content and the post-accept file content, then restoring the
fix).

## What's deliberately out of scope for this pass

The parent issue also asked for (a) changing `fake_acp_agent.sh`'s *shared*
defaults (fragment diffs, terminal-auth `args`, `loadSession`/
`embeddedContext`/`image` capabilities) so every existing ACP test exercises
the realistic shape by default, and (b) a broader corpus covering
multi-file turns, cancel, and terminal-auth re-init specifically. Both are
left for a follow-up: (a) has a wide blast radius (~15+ existing tests key
off the fixture's current default shapes) that deserves its own focused
pass rather than riding along with new infrastructure, and (b) is
straightforward to add once this scaffolding exists — new scenarios are
just new `.transcript` files plus a `record_acp_transcript` invocation, no
new plumbing.
