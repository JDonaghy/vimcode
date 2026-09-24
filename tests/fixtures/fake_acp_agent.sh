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
#   - initialize        -> canned success result, with agentInfo.sawReadCap /
#                           .sawWriteCap echoing back whether the request
#                           actually carried clientCapabilities.fs.
#                           {readTextFile,writeTextFile}: true (#954, ACP-3)
#                           — so a test can confirm the capability reached
#                           the wire, not just the client's own bookkeeping.
#                           If $ACP_FAKE_EMIT_GARBAGE is set, a non-JSON line
#                           is printed to stdout immediately before the real
#                           response, to prove the reader skips it without
#                           desyncing. If $ACP_FAKE_DIE_AFTER_INIT is set,
#                           this process exits right after replying.
#   - session/new        -> canned sessionId "sess-1". If
#                           $ACP_FAKE_SESSION_NEW_ERROR is set, replies with a
#                           JSON-RPC error instead (agent stays alive,
#                           doesn't exit) — for the non-fatal-handshake-error
#                           regression: `RequestFailed` for a method other
#                           than session/prompt must still clear the panel's
#                           busy state (#952 review finding). If
#                           $ACP_FAKE_SESSION_MODES is set (#956, ACP-5), the
#                           result also carries a `modes` field with two
#                           modes ("code" current, "plan" available) — for a
#                           test to drive `:AiMode`/`session/set_mode`
#                           against.
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
#                           reply still reaches this process. With
#                           $ACP_FAKE_FS_READ_PATH set (#954, ACP-3): emits a
#                           scripted fs/read_text_file request (fixed id
#                           9010) for that path, BLOCKS for the reply, then
#                           emits an agent_message_chunk with text
#                           "read:<content>" — so a test can drive a real
#                           `path -> reply` round trip through the engine's
#                           actual buffer-first dispatch (not just the
#                           transport layer) and assert on what came back,
#                           by reading the transcript. With
#                           $ACP_FAKE_FS_WRITE_PATH (and optionally
#                           $ACP_FAKE_FS_WRITE_CONTENT, default "written by
#                           acp") set instead: emits a scripted
#                           fs/write_text_file request (fixed id 9011) for
#                           that path/content, BLOCKS for the reply, then
#                           emits an agent_message_chunk with text
#                           "write:ok" or "write:error:<message>" depending
#                           on whether the reply was a JSON-RPC error —
#                           same "drive it for real, read the transcript"
#                           shape as the read case. With $ACP_FAKE_PLAN set
#                           (#956, ACP-5): emits an available_commands_update
#                           (two commands, "commit" and "compact", sharing
#                           the "co" prefix on purpose so a test can confirm
#                           completion narrows on it), then TWO successive
#                           "plan" updates back to back — the first with one
#                           in_progress entry, the second (a full
#                           replacement, not a delta) with that entry marked
#                           completed plus a new pending one — so a test can
#                           confirm exactly one plan renders afterward,
#                           reflecting the second update. Then a usage_update
#                           (inputTokens/outputTokens/totalCostUsd), then the
#                           usual agent_message_chunk + end_turn, no fs/*
#                           request (out of scope for this scenario).
#   - session/cancel     -> notification, silently acknowledged (no reply).
#   - session/set_mode   -> replies with an empty result, then emits a
#                           current_mode_update notification carrying the
#                           same modeId the request asked for (#956, ACP-5)
#                           — proving the displayed mode follows the
#                           notification, not the request succeeding.
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
      # Echo back whether this client's own `initialize` request actually
      # carried the fs/* clientCapabilities (#954, ACP-3) — a substring
      # check on the raw request line, not a real JSON parse (this script
      # stays jq/python/node-free deliberately), so a test can confirm the
      # capability flags reached the wire instead of trusting the client's
      # own bookkeeping.
      saw_read=false
      saw_write=false
      case "$line" in *'"readTextFile":true'*) saw_read=true ;; esac
      case "$line" in *'"writeTextFile":true'*) saw_write=true ;; esac
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{},"agentInfo":{"name":"fake-acp-agent","version":"0.0.1","sawReadCap":%s,"sawWriteCap":%s},"authMethods":[]}}\n' "$id" "$saw_read" "$saw_write"
      if [ -n "$ACP_FAKE_DIE_AFTER_INIT" ]; then
        exit 7
      fi
      ;;
    *'"method":"session/new"'*)
      id=$(extract_id "$line")
      if [ -n "$ACP_FAKE_SESSION_NEW_ERROR" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"cwd not permitted"}}\n' "$id"
      elif [ -n "$ACP_FAKE_SESSION_MODES" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"sess-1","modes":{"currentModeId":"code","availableModes":[{"id":"code","name":"Code"},{"id":"plan","name":"Plan"}]}}}\n' "$id"
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
      elif [ -n "$ACP_FAKE_PLAN" ]; then
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"available_commands_update","availableCommands":[{"name":"commit","description":"Commit staged changes"},{"name":"compact","description":"Compact the conversation"}]}}}\n'
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"plan","entries":[{"content":"Write the fix","status":"in_progress"}]}}}\n'
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"plan","entries":[{"content":"Write the fix","status":"completed"},{"content":"Add tests","status":"pending"}]}}}\n'
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"usage_update","usage":{"inputTokens":120,"outputTokens":45,"totalCostUsd":0.0067}}}}\n'
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Plan ready"}}}}\n'
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
      elif [ -n "$ACP_FAKE_FS_READ_PATH" ]; then
        printf '{"jsonrpc":"2.0","id":9010,"method":"fs/read_text_file","params":{"sessionId":"sess-1","path":"%s"}}\n' "$ACP_FAKE_FS_READ_PATH"
        # Park: block until the client answers request 9010 out of band.
        read -r fsreply
        content=$(printf '%s' "$fsreply" | sed -n 's/.*"content":"\([^"]*\)".*/\1/p')
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"read:%s"}}}}\n' "$content"
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
      elif [ -n "$ACP_FAKE_FS_WRITE_PATH" ]; then
        write_content="${ACP_FAKE_FS_WRITE_CONTENT:-written by acp}"
        printf '{"jsonrpc":"2.0","id":9011,"method":"fs/write_text_file","params":{"sessionId":"sess-1","path":"%s","content":"%s"}}\n' "$ACP_FAKE_FS_WRITE_PATH" "$write_content"
        # Park: block until the client answers request 9011 out of band.
        read -r fsreply
        case "$fsreply" in
          *'"error"'*)
            message=$(printf '%s' "$fsreply" | sed -n 's/.*"message":"\([^"]*\)".*/\1/p')
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"write:error:%s"}}}}\n' "$message"
            ;;
          *)
            printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"write:ok"}}}}\n'
            ;;
        esac
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
    *'"method":"session/set_mode"'*)
      id=$(extract_id "$line")
      mode_id=$(printf '%s' "$line" | sed -n 's/.*"modeId":"\([^"]*\)".*/\1/p')
      printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"current_mode_update","currentModeId":"%s"}}}\n' "$mode_id"
      ;;
    *)
      echo "fake-acp-agent: unrecognized line: $line" 1>&2
      ;;
  esac
done

echo "fake-acp-agent: stdin closed, exiting" 1>&2
