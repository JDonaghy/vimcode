#!/bin/sh
# Fake NDJSON ACP echo agent — the shared fixture the whole ACP track (#531)
# depends on to prove itself deterministically. CI has no Node and no real
# agent login, so no ACP slice may depend on a real adapter (#951).
#
# This is not a protocol implementation: it matches request lines by a
# simple substring `case`, extracts the numeric JSON-RPC id with `sed`, and
# prints hand-built response lines. That keeps it running on plain
# `/bin/sh` with no jq/python/node dependency, deliberately.
#
# Scripted behaviour (see src/core/acp.rs's tests for what exercises each):
#   - initialize        -> canned success result. If $ACP_FAKE_EMIT_GARBAGE
#                           is set, a non-JSON line is printed to stdout
#                           immediately before the real response, to prove
#                           the reader skips it without desyncing. If
#                           $ACP_FAKE_DIE_AFTER_INIT is set, this process
#                           exits right after replying.
#   - session/new        -> canned sessionId "sess-1". If
#                           $ACP_FAKE_SESSION_NEW_ERROR is set, replies with a
#                           JSON-RPC error instead (agent stays alive,
#                           doesn't exit) — for the non-fatal-handshake-error
#                           regression: `RequestFailed` for a method other
#                           than session/prompt must still clear the panel's
#                           busy state (#952 review finding).
#   - session/prompt     -> emits a session/update "agent_thought_chunk"
#                           notification, then two "agent_message_chunk"
#                           notifications (split across two lines, to prove
#                           chunk *streaming* rather than one whole-turn
#                           blob — ACP-1, #952), both using the real ACP v1
#                           wire shape (`sessionUpdate` tag + `content.text`,
#                           not the placeholder `kind`/`text` shape ACP-0
#                           used before any slice read the field values).
#                           Then, unless $ACP_FAKE_NO_TOOL_REQUEST is set, a
#                           scripted agent->client request (fixed id 9001,
#                           method fs/read_text_file) that BLOCKS reading one
#                           more line before replying with stopReason:
#                           end_turn — proving a reply written out of band
#                           via AcpClient::respond_to_client_request actually
#                           reaches the agent and unblocks it, not just that
#                           the client-side bookkeeping looks right. With
#                           $ACP_FAKE_NO_TOOL_REQUEST set, replies with
#                           stopReason: end_turn immediately after the
#                           chunks — for ACP-1 tests that only exercise the
#                           streaming/chunk-mapping path, not the fs/*
#                           bridge (out of scope until a later ACP slice).
#                           With $ACP_FAKE_REQUEST_PERMISSION set (ACP-2,
#                           #953) instead: emits a scripted
#                           session/request_permission request (fixed id
#                           9002, toolCall title "Edit src/main.rs", kind
#                           "edit", one location) offering allow_once/
#                           allow_always/reject_once options, then BLOCKS
#                           reading one line before replying end_turn — on
#                           *every* session/prompt call in this agent
#                           process, not just the first, so a test can drive
#                           two turns in the same session and confirm the
#                           second one's request never needs a dialog
#                           (client-side allow_always memory) while still
#                           proving the reply actually reaches this process
#                           each time. With $ACP_FAKE_DIE_DURING_PERMISSION
#                           set: emits that same request_permission request
#                           and exits immediately without reading a reply —
#                           for the "agent dies with a permission dialog
#                           open" acceptance criterion (must not hang, must
#                           not write to the now-dead stdin). With
#                           $ACP_FAKE_MALFORMED_REQUEST_PERMISSION set:
#                           emits a session/request_permission request
#                           (fixed id 9003) with no "options" array —
#                           malformed per the ACP v1 schema (there is
#                           nothing a human could select) — then BLOCKS
#                           reading one line before replying end_turn, same
#                           shape as the well-formed variant, so a test can
#                           confirm the client answers with a JSON-RPC error
#                           immediately (never opens a dialog) and that
#                           reply still reaches this process.
#   - session/cancel     -> notification, silently acknowledged (no reply).
#   - anything else      -> logged to stderr, ignored.
#
# Always logs a startup line to stderr, to prove stderr noise is never
# mistaken for protocol traffic.

echo "fake-acp-agent: starting" 1>&2

extract_id() {
  printf '%s' "$1" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p'
}

while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      id=$(extract_id "$line")
      if [ -n "$ACP_FAKE_EMIT_GARBAGE" ]; then
        echo 'this line is not JSON and must be skipped by the reader'
      fi
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{},"agentInfo":{"name":"fake-acp-agent","version":"0.0.1"},"authMethods":[]}}\n' "$id"
      if [ -n "$ACP_FAKE_DIE_AFTER_INIT" ]; then
        exit 7
      fi
      ;;
    *'"method":"session/new"'*)
      id=$(extract_id "$line")
      if [ -n "$ACP_FAKE_SESSION_NEW_ERROR" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"cwd not permitted"}}\n' "$id"
      else
        printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"sess-1"}}\n' "$id"
      fi
      ;;
    *'"method":"session/prompt"'*)
      id=$(extract_id "$line")
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"pondering the question"}}}}\n'
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hello"}}}}\n'
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":" world"}}}}\n'
      if [ -n "$ACP_FAKE_NO_TOOL_REQUEST" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
      elif [ -n "$ACP_FAKE_REQUEST_PERMISSION" ]; then
        printf '{"jsonrpc":"2.0","id":9002,"method":"session/request_permission","params":{"sessionId":"sess-1","toolCall":{"title":"Edit src/main.rs","kind":"edit","locations":[{"path":"src/main.rs","line":42}]},"options":[{"optionId":"allow-once","name":"Allow Once","kind":"allow_once"},{"optionId":"allow-always","name":"Always Allow","kind":"allow_always"},{"optionId":"reject-once","name":"Reject","kind":"reject_once"}]}}\n'
        # Park: block until the client answers request 9002 out of band.
        read -r _reply
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
      elif [ -n "$ACP_FAKE_DIE_DURING_PERMISSION" ]; then
        printf '{"jsonrpc":"2.0","id":9002,"method":"session/request_permission","params":{"sessionId":"sess-1","toolCall":{"title":"Edit src/main.rs","kind":"edit","locations":[{"path":"src/main.rs","line":42}]},"options":[{"optionId":"allow-once","name":"Allow Once","kind":"allow_once"},{"optionId":"allow-always","name":"Always Allow","kind":"allow_always"},{"optionId":"reject-once","name":"Reject","kind":"reject_once"}]}}\n'
        exit 9
      elif [ -n "$ACP_FAKE_MALFORMED_REQUEST_PERMISSION" ]; then
        printf '{"jsonrpc":"2.0","id":9003,"method":"session/request_permission","params":{"sessionId":"sess-1","toolCall":{"title":"Edit src/main.rs","kind":"edit"}}}\n'
        # Park: block until the client answers request 9003 out of band —
        # the client should answer immediately with a JSON-RPC error since
        # there is no "options" array to select from, never opening a
        # dialog first.
        read -r _reply
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
      else
        printf '{"jsonrpc":"2.0","id":9001,"method":"fs/read_text_file","params":{"sessionId":"sess-1","path":"/tmp/fake.txt"}}\n'
        # Park: block until the client answers request 9001 out of band.
        read -r _reply
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
      fi
      ;;
    *'"method":"session/cancel"'*)
      : # fire-and-forget notification, no reply expected
      ;;
    *)
      echo "fake-acp-agent: unrecognized line: $line" 1>&2
      ;;
  esac
done

echo "fake-acp-agent: stdin closed, exiting" 1>&2
