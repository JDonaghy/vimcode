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
#   - session/prompt     -> (#1449) if $ACP_FAKE_CAPTURE_PROMPT_TO names a
#                           file, the raw request line is appended to it
#                           first, unconditionally — lets a test assert on
#                           the actual `prompt` content-block array
#                           (`resource_link` for an attached buffer/
#                           mention, plain `text` otherwise) via
#                           `serde_json` on the Rust side, without teaching
#                           this jq/python/node-free script to parse JSON.
#                           Then, emits a session/update "agent_thought_chunk"
#                           notification, then two "agent_message_chunk"
#                           notifications (split across two lines, to prove
#                           chunk *streaming* rather than one whole-turn
#                           blob — ACP-1, #952), both using the real ACP v1
#                           wire shape (`sessionUpdate` tag + `content.text`,
#                           not the placeholder `kind`/`text` shape ACP-0
#                           used before any slice read the field values).
#                           The first message chunk reads "Hello", or
#                           "Hello_$ACP_FAKE_AGENT_LABEL" (one unbroken
#                           token, so it survives panel word-wrap intact) if
#                           that env var is set (#958, ACP-7) — lets a test
#                           run this exact same binary as two *different*
#                           registry entries (only `env` differs) and tell
#                           their replies apart, proving the multi-agent
#                           registry is a config fact, not a Rust
#                           special-case.
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
#                           request (out of scope for this scenario). With
#                           $ACP_FAKE_TOOL_CALL_STATUS_ONLY set (#955,
#                           ACP-4): the same tool_call/tool_call_update
#                           status-transition shape as $ACP_FAKE_TOOL_CALL
#                           below, but with no diff content, so a test can
#                           see the transcript's status glyph without the
#                           change-review surface covering it. With
#                           $ACP_FAKE_TOOL_CALL set (#955, ACP-4): emits a
#                           "tool_call" (id "tc-1", pending), a
#                           "tool_call_update" moving it to in_progress,
#                           then a second "tool_call_update" moving it to
#                           completed AND attaching a "diff" content block
#                           (path "src/main.rs") on that same update — so a
#                           test can confirm both the status transition and
#                           the change-review surface opening come from a
#                           tool_call_update, not just the initial
#                           tool_call.
#   - session/cancel     -> notification, silently acknowledged (no reply).
#   - session/set_mode   -> replies with an empty result, then emits a
#                           current_mode_update notification carrying the
#                           same modeId the request asked for (#956, ACP-5)
#                           — proving the displayed mode follows the
#                           notification, not the request succeeding.
#   - authenticate       -> (#957, ACP-6) if $ACP_FAKE_AUTH_FAIL is set,
#                           replies with a JSON-RPC error; otherwise replies
#                           with an empty success result.
#   - anything else      -> logged to stderr, ignored.
#
# #957 (ACP-6): `initialize`'s `authMethods`. With $ACP_FAKE_AUTH_METHODS
# set, the initialize reply's `authMethods` always includes one `type:
# "agent"` entry ("api-key" / "API Key"), and additionally includes one
# `type: "terminal"` entry ("claude-ai-login" / "Claude Subscription") —
# but *only* if this request's own `clientCapabilities.auth.terminal` was
# `true` on the wire (a real substring check on the request line, same
# style as the sawReadCap/sawWriteCap echo above), also echoed back as
# agentInfo.sawAuthTerminalCap regardless of $ACP_FAKE_AUTH_METHODS. This
# makes "the terminal method is absent when the capability isn't
# advertised" a real, request-driven branch rather than a hardcoded stub —
# see `core::acp`'s `parse_auth_methods` tests for the client-side half of
# that contract (an authMethods array with no `type: "terminal"` entry at
# all yields no `AcpAuthMethodKind::Terminal` results).
#
# #1450: `initialize`'s `agentCapabilities.promptCapabilities.embeddedContext`
# is `false`/absent (`agentCapabilities: {}`) unless $ACP_FAKE_EMBEDDED_CONTEXT
# is set, in which case it's `true` — lets a test drive both branches of
# `Engine::acp_prompt_content_blocks`'s range-attachment content-block choice
# (an embedded `resource` block with the exact buffer text vs. a
# `resource_link` + fenced-text fallback) against the same fixture.
#
# #957 (ACP-6) interactive terminal-auth login. When this script's own
# stdin is a real TTY — i.e. it was launched by `Engine::
# acp_launch_terminal_login`'s `TerminalSession` (a real PTY) rather than
# piped NDJSON — it skips the JSON-RPC loop entirely and simulates an
# interactive CLI login instead, the same behavioral split a real ACP
# adapter would make on `isatty(stdin)`. `$1` (the first CLI arg, carried
# unmodified through `settings.acp_agent_command`'s own trailing word — no
# environment variable or other test-only side channel needed, since both
# the NDJSON spawn and the interactive re-spawn share that one string)
# selects the outcome, and is optional: "fail" simulates a declined/failed
# login (exit 1); "hang" skips this shortcut entirely and falls into the
# ordinary NDJSON-shaped `read` loop below, which blocks forever on this
# real TTY instead of a piped-closed one — simulating a login a human
# abandons by closing the pane before it ever exits, for a test to race
# against with `Engine::terminal_close_active_tab`. "succeed-slow" is the
# same success path plus a two-second `sleep` before exiting — for a
# `TuiDriver` black-box test (`tui_main::shell_app::tests::
# ai_panel_terminal_auth_choice_opens_visible_login_pane_and_resumes_session_via_shell_app`)
# that needs a real window to poll-and-render the login pane's own painted
# PTY output ("...login succeeded") before the pane closes itself and is
# reaped, proving the pane actually became visible/painted rather than
# only asserting `terminal_panes.len()` / `acp_authenticated` state (#957
# review). GTK's twin test uses a different visibility proof instead (the
# bottom panel's tab-strip chrome, not PTY cell text — see that test's own
# doc comment for why) so it doesn't need this arg.
#
# #1444: after that optional control word, every remaining arg must be
# exactly the "claude-ai-login" method's own `args` above
# (`--cli auth login --claudeai`) — `Engine::acp_launch_terminal_login` is
# responsible for appending them to the resolved command
# (`AcpAuthMethod::args`), and this fixture *refuses to "log in"* (exit 1,
# distinct stderr message) if they are missing or wrong, so a regression
# that drops them (the exact bug #1444 reported: the bare command was run,
# which a real adapter answers by starting its NDJSON server and never
# logging in at all) fails every test below this comment, not just a
# dedicated one.
if [ -t 0 ]; then
  ctrl=""
  case "$1" in
    fail | hang | succeed-slow)
      ctrl="$1"
      shift
      ;;
  esac
  if [ "$ctrl" != "hang" ]; then
    if [ "$1 $2 $3 $4" != "--cli auth login --claudeai" ] || [ -n "$5" ]; then
      echo "fake-acp-agent: interactive login refused: expected args '--cli auth login --claudeai', got '$*'" 1>&2
      exit 1
    fi
    if [ "$ctrl" = "fail" ]; then
      echo "fake-acp-agent: interactive login failed" 1>&2
      exit 1
    fi
    echo "fake-acp-agent: interactive login succeeded"
    if [ "$ctrl" = "succeed-slow" ]; then
      sleep 2
    fi
    exit 0
  fi
fi

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
      saw_auth_terminal=false
      case "$line" in *'"readTextFile":true'*) saw_read=true ;; esac
      case "$line" in *'"writeTextFile":true'*) saw_write=true ;; esac
      case "$line" in *'"auth":{"terminal":true}'*) saw_auth_terminal=true ;; esac
      # #957 (ACP-6): authMethods is empty unless $ACP_FAKE_AUTH_METHODS is
      # set — every pre-#957 test relies on that default so a fresh
      # `session/new` proceeds immediately, unauthenticated. When set, the
      # "agent"-type entry is unconditional; the "terminal"-type entry is
      # gated on $saw_auth_terminal, i.e. on what this request actually
      # carried — see this script's top-of-file doc for why that matters.
      auth_methods='[]'
      if [ -n "$ACP_FAKE_AUTH_METHODS" ]; then
        if [ "$saw_auth_terminal" = "true" ]; then
          auth_methods='[{"id":"api-key","name":"API Key","type":"agent"},{"id":"claude-ai-login","name":"Claude Subscription","type":"terminal","args":["--cli","auth","login","--claudeai"]}]'
        else
          auth_methods='[{"id":"api-key","name":"API Key","type":"agent"}]'
        fi
      fi
      # #1450: $ACP_FAKE_EMBEDDED_CONTEXT set -> agentCapabilities carries
      # promptCapabilities.embeddedContext:true, so a test can drive the
      # "agent understands `resource` content blocks" branch of
      # `Engine::acp_prompt_content_blocks`. Unset (every pre-#1450 test)
      # keeps the empty `{}` every prior slice relies on, which parses as
      # all-`false` per `parse_prompt_capabilities`'s doc.
      agent_caps='{}'
      if [ -n "$ACP_FAKE_EMBEDDED_CONTEXT" ]; then
        agent_caps='{"promptCapabilities":{"embeddedContext":true}}'
      fi
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":%s,"agentInfo":{"name":"fake-acp-agent","version":"0.0.1","sawReadCap":%s,"sawWriteCap":%s,"sawAuthTerminalCap":%s},"authMethods":%s}}\n' "$id" "$agent_caps" "$saw_read" "$saw_write" "$saw_auth_terminal" "$auth_methods"
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
      # #1449: when $ACP_FAKE_CAPTURE_PROMPT_TO names a file, the whole
      # raw `session/prompt` request line is appended to it before any
      # other handling — the client-side content-block shape (`resource_
      # link` for the attached buffer / `@`-mention, plain `text` for
      # everything else) is easiest to assert on from the Rust side by
      # reading this file and running it through `serde_json`, rather than
      # teaching this jq/python/node-free `/bin/sh` script to parse JSON
      # itself. Append (not overwrite) so a test that sends more than one
      # message in the same session can inspect each turn's params.
      if [ -n "$ACP_FAKE_CAPTURE_PROMPT_TO" ]; then
        printf '%s\n' "$line" >> "$ACP_FAKE_CAPTURE_PROMPT_TO"
      fi
      # #958 (ACP-7): $ACP_FAKE_AGENT_LABEL, if set, is folded into the
      # first message chunk so a test can run this exact same script as two
      # differently-configured `settings.acp_agents` registry entries and
      # tell their replies apart on screen — see this file's top-of-file
      # doc.
      # Deliberately one unbroken token when the label is set (no spaces) —
      # a multi-word greeting can word-wrap across two rendered rows in a
      # narrow panel, which would break a caller's `screen_contains` check
      # on the whole string (see `ai_panel_shows_actionable_message_when_
      # agent_binary_is_missing_via_shell_app`'s doc for the same reasoning).
      hello_text="Hello"
      if [ -n "$ACP_FAKE_AGENT_LABEL" ]; then
        hello_text="Hello_${ACP_FAKE_AGENT_LABEL}"
      fi
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"pondering the question"}}}}\n'
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$hello_text"
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
      elif [ -n "$ACP_FAKE_TOOL_CALL_STATUS_ONLY" ]; then
        # #955 (ACP-4): the same tool-call/status-transition shape as
        # $ACP_FAKE_TOOL_CALL below, but with NO diff content — so a test
        # can observe the transcript's rendered status glyph without the
        # change-review surface (a full-viewport overlay) painting over
        # it.
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"tool_call","toolCallId":"tc-1","title":"Run the tests","kind":"execute","status":"pending"}}}\n'
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"tc-1","status":"in_progress"}}}\n'
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"tc-1","status":"completed"}}}\n'
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
      elif [ -n "$ACP_FAKE_TOOL_CALL" ]; then
        # #955 (ACP-4): announce a tool call, transition it through
        # in_progress -> completed via tool_call_update, then attach a
        # `diff` content block on the *same* completed update — proving
        # a `tool_call_update` (not just the initial `tool_call`) is what
        # opens the change-review surface. `$ACP_FAKE_TOOL_CALL_PATH`
        # (default "src/main.rs") lets a test point the diff at a real
        # scratch file so an "accept" round-trip has somewhere safe to
        # write.
        tc_path="${ACP_FAKE_TOOL_CALL_PATH:-src/main.rs}"
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"tool_call","toolCallId":"tc-1","title":"Edit %s","kind":"edit","status":"pending","locations":[{"path":"%s","line":1}]}}}\n' "$tc_path" "$tc_path"
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"tc-1","status":"in_progress"}}}\n'
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"sess-1","update":{"sessionUpdate":"tool_call_update","toolCallId":"tc-1","status":"completed","content":[{"type":"diff","path":"%s","oldText":"old line\\n","newText":"new line\\n"}]}}}\n' "$tc_path"
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
    *'"method":"authenticate"'*)
      id=$(extract_id "$line")
      if [ -n "$ACP_FAKE_AUTH_FAIL" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"invalid credentials"}}\n' "$id"
      else
        printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
      fi
      ;;
    *)
      echo "fake-acp-agent: unrecognized line: $line" 1>&2
      ;;
  esac
done

echo "fake-acp-agent: stdin closed, exiting" 1>&2
