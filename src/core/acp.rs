//! ACP (Agent Client Protocol) transport — NDJSON JSON-RPC 2.0 over a
//! subprocess's stdio, plus a minimal session lifecycle (#951, ACP-0).
//!
//! This is the foundation of the ACP track (epic #531). It ships the
//! transport and the client<->agent session lifecycle only — **no UI**.
//! Later slices build the AI panel state machine and rendering on top of
//! [`AcpEvent`] and [`Engine::poll_acp`](crate::core::Engine::poll_acp).
//!
//! ## Why this is not `lsp.rs` reuse
//!
//! `src/core/lsp.rs` is the closest thing in the tree and the right
//! *shape* to copy, but two specifics do not carry over:
//!
//! 1. **Framing.** ACP is one UTF-8 JSON-RPC message per line on stdio
//!    ([transports](https://agentclientprotocol.com/protocol/transports)),
//!    not LSP's `Content-Length` header framing. See [`encode_ndjson_line`]
//!    and [`classify_line`].
//! 2. **Agent -> client requests are not blanket-answered.** LSP's reader
//!    thread answers every server->client request with `result: null`
//!    without reading the method. An ACP client is *dominated* by
//!    agent->client requests (`session/request_permission`,
//!    `fs/read_text_file`, `fs/write_text_file`) and several must **park**
//!    until a human or the UI thread answers. This module dispatches such
//!    requests by method name via [`AcpEvent::ClientRequest`] and lets the
//!    caller answer them later, out of band, with
//!    [`AcpClient::respond_to_client_request`] — the reply is written
//!    through `stdin`, which is [`Arc<Mutex<_>>`] and shared with the
//!    reader thread for exactly this reason (see `LspServer::stdin`, and
//!    contrast with `DapServer::stdin`, a `BufWriter` *not* shared with its
//!    reader, which is why the DAP client cannot answer adapter requests —
//!    do not repeat that here).
//!
//! ## ACP-6: `auth.terminal` and the terminal-login flow
//!
//! `initialize`'s `clientCapabilities.auth.terminal: true` (advertised
//! unconditionally, see [`AcpClient::initialize`]) is what makes an agent's
//! `initialize` response allowed to offer a `type: "terminal"` auth method
//! at all — e.g. `claude-ai-login` / "Claude Subscription" from
//! `@agentclientprotocol/claude-agent-acp`. Omitting the capability is how
//! a client opts out of subscription login and forces API-key-only; this
//! client deliberately does not do that.
//!
//! ### How terminal auth differs from `authenticate`
//!
//! Two distinct mechanisms, and conflating them is the trap the issue
//! calls out:
//!
//! - `type: "agent"` methods -> call [`AcpClient::authenticate`]
//!   (`authenticate {methodId}`).
//! - `type: "terminal"` methods -> **do not** call `authenticate` at all.
//!   The client runs the agent's own command line attached to a real TTY
//!   so its CLI can drive an interactive login, then re-initializes once
//!   that process exits (`Engine::acp_launch_terminal_login` /
//!   `Engine::acp_finish_terminal_login`, `src/core/engine/terminal_ops.rs`
//!   / `acp_ops.rs`). See [`AcpAuthMethodKind`] for the wire-level
//!   classification.
//!
//! ## Standing commitments (apply to the whole ACP track, not just this file)
//!
//! - Protocol version is pinned at `1` (a draft v2 restructures capabilities
//!   and drops `fs/*`/`terminal/*` in favour of MCP-over-ACP).
//! - Capabilities advertised in `initialize` are **only** what later slices
//!   actually implement. Omitted means unsupported. As of #954 (ACP-3) that
//!   is `fs.readTextFile` and `fs.writeTextFile` — see [`AcpClient::initialize`].
//! - `terminal/*` is out of scope (optional in v1, removed in the v2 draft).
//! - No async runtime: everything here is sync threads + `mpsc`, matching
//!   `lsp.rs`/`dap.rs` and vimcode's <=250ms sync tick.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read as IoRead, Write as IoWrite};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

/// ACP protocol version this client speaks. Pinned per the ACP track's
/// standing commitment — see the module doc.
pub const PROTOCOL_VERSION: i64 = 1;

/// Cap on events drained from the agent per `poll()` call. Session-update
/// notifications can stream at a high rate during a prompt turn (chunked
/// text); capping keeps a single idle tick bounded, matching
/// `LspManager::poll_events`'s `max_events = 50`.
const MAX_EVENTS_PER_POLL: usize = 50;

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Events emitted by an ACP agent subprocess, drained via [`AcpClient::poll`].
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// Response to our `initialize` request.
    Initialized {
        request_id: i64,
        protocol_version: i64,
        agent_capabilities: serde_json::Value,
        agent_info: serde_json::Value,
        auth_methods: Vec<serde_json::Value>,
    },
    /// Response to our `session/new` request.
    SessionCreated {
        request_id: i64,
        session_id: String,
        modes: Option<serde_json::Value>,
        config_options: Option<serde_json::Value>,
    },
    /// Response to our `session/prompt` request — the turn has ended.
    PromptStopped {
        request_id: i64,
        stop_reason: String,
    },
    /// A `session/update` notification streamed during a prompt turn.
    /// `update` is the notification's whole `params` object, i.e.
    /// `{"sessionId": ..., "update": {"sessionUpdate": "...", "content":
    /// {...}}}` — callers that want the tagged-union chunk payload (as
    /// [`session_update_chunk`] parses) must read `update.get("update")`
    /// first; `session_id` is hoisted out already for convenience/dispatch.
    SessionUpdate {
        session_id: String,
        update: serde_json::Value,
    },
    /// An agent -> client request, dispatched by method name. **Not**
    /// auto-answered. The id is recorded in `pending_client_requests` until
    /// [`AcpClient::respond_to_client_request`] is called with it — a
    /// handler is free to return immediately (call it inline) or park
    /// (return from the event loop and call it later, out of band).
    ClientRequest {
        request_id: i64,
        method: String,
        params: serde_json::Value,
    },
    /// One of our own client -> agent requests came back with a JSON-RPC
    /// `error`, or a response with neither `result` nor `error`.
    RequestFailed {
        request_id: i64,
        method: String,
        message: String,
    },
    /// The agent process exited (EOF on stdout, or the pipe broke).
    AgentExited {
        stderr: String,
        was_initialized: bool,
    },
    /// Response to our `authenticate` request (#957, ACP-6) — a `type:
    /// "agent"` auth method succeeded. A failure comes through
    /// [`AcpEvent::RequestFailed`] instead (method `"authenticate"`), same
    /// as every other client -> agent request.
    Authenticated { request_id: i64 },
}

// ---------------------------------------------------------------------------
// Pure helpers — NDJSON framing
// ---------------------------------------------------------------------------

/// Encode a JSON-RPC message as one NDJSON line: compact JSON followed by
/// `\n`. ACP puts exactly one message per line; there is no header framing
/// to get wrong the way LSP's `Content-Length` is (#951).
pub fn encode_ndjson_line(body: &serde_json::Value) -> Vec<u8> {
    let mut s = body.to_string();
    s.push('\n');
    s.into_bytes()
}

/// The shape a decoded NDJSON line falls into, once we know whether it
/// carries `method`/`id`. Pure classification — no I/O, so it is trivially
/// unit-testable against malformed input without a subprocess.
#[derive(Debug, PartialEq)]
pub(crate) enum ParsedLine {
    /// Agent -> client request: has both `method` and `id`.
    AgentToClientRequest {
        id: i64,
        method: String,
        params: serde_json::Value,
    },
    /// Agent -> client notification: has `method`, no `id`.
    Notification {
        method: String,
        params: serde_json::Value,
    },
    /// Response to one of our client -> agent requests: has `id`, no `method`.
    Response {
        id: i64,
        result: Option<serde_json::Value>,
        error: Option<serde_json::Value>,
    },
    /// Blank line, not valid JSON, or valid JSON with neither `method` nor
    /// `id` (e.g. stray agent stdout noise from a misbehaving agent).
    /// Skipped by the reader without desyncing — NDJSON resyncs every line.
    Unusable,
}

/// Classify one line of agent stdout. Never panics on malformed input.
pub(crate) fn classify_line(line: &str) -> ParsedLine {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return ParsedLine::Unusable;
    }
    let json: serde_json::Value = match serde_json::from_str(trimmed) {
        Ok(v) => v,
        Err(_) => return ParsedLine::Unusable,
    };

    let method = json.get("method").and_then(|m| m.as_str());
    // Accept both a numeric id and a stringified numeric id, same
    // tolerance `lsp.rs`'s reader applies (some servers echo ids as
    // strings).
    let id = json.get("id").and_then(|v| {
        v.as_i64()
            .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
    });
    let params = json
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    match (method, id) {
        (Some(m), Some(id)) => ParsedLine::AgentToClientRequest {
            id,
            method: m.to_string(),
            params,
        },
        (Some(m), None) => ParsedLine::Notification {
            method: m.to_string(),
            params,
        },
        (None, Some(id)) => ParsedLine::Response {
            id,
            result: json.get("result").cloned(),
            error: json.get("error").cloned(),
        },
        (None, None) => ParsedLine::Unusable,
    }
}

/// Best-effort absolute path string for `session/new`'s `cwd`, which ACP
/// requires to be absolute. Falls back to the path as given if it can't be
/// canonicalized (e.g. it doesn't exist yet).
fn absolute_path_string(path: &Path) -> String {
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    abs.to_string_lossy().into_owned()
}

/// Split a user-configured ACP agent command line (`settings.
/// acp_agent_command`) into `argv` for [`AcpClient::spawn`].
///
/// Supports plain whitespace-separated tokens and double-quoted segments
/// (`"..."`) for a single arg containing spaces (e.g. a path). No other
/// shell syntax — `$VAR` expansion, globs, pipes, single quotes — is
/// interpreted; this is argv construction for `Command::args`, not a shell
/// (`core::terminal::shell_command` is the seam for actual `sh -c` use, and
/// does not apply here per #1255's reasoning: this is a fixed program
/// invocation, not a user-supplied shell command string).
///
/// **This same `settings.acp_agent_command` string is parsed a second,
/// different way for the `type: "terminal"` auth login path** (#957,
/// ACP-6): `Engine::acp_launch_terminal_login`
/// (`src/core/engine/terminal_ops.rs`) writes it verbatim into a real
/// interactive shell via `build_acp_auth_wrapper`, where it *is*
/// shell-interpreted (`$VAR`, backticks, `;`, pipes, …), unlike this
/// splitter. A command containing shell metacharacters would therefore
/// behave differently between the two paths for the same setting. The
/// command is operator-configured, not attacker input, so this is a
/// latent correctness surprise rather than a security bug — worth
/// revisiting (e.g. quoting the wrapper's injected command) if it ever
/// causes real confusion, but out of scope for #957 itself.
pub fn parse_agent_command(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in cmd.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// One entry in the user-configured multi-agent registry
/// (`settings.acp_agents`, #958 ACP-7). Bringing up a *second* ACP agent is
/// meant to be a pure config fact — a second `AcpAgentProfile` spawned
/// through the exact same [`AcpClient::spawn_with_env`] call the single-agent
/// `acp_agent_command` path already used — never a new branch on which
/// agent is active anywhere in this file or `Engine`. See
/// `Engine::acp_resolve_agent_launch` (`src/core/engine/acp_ops.rs`), the
/// one place that reads this struct.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AcpAgentProfile {
    /// Display/lookup name, matched case-insensitively by `:AiAgent
    /// <name>`. Must be unique within `settings.acp_agents`; the first
    /// match wins if it isn't.
    pub name: String,
    /// Command line, parsed the same way as `acp_agent_command`
    /// ([`parse_agent_command`]) — plain whitespace tokens plus
    /// double-quoted segments.
    pub command: String,
    /// Working directory for `session/new`'s `cwd`. Empty (the default)
    /// falls back to the workspace root / process cwd, same as the
    /// single-agent path (`Engine::acp_workspace_cwd`).
    #[serde(default)]
    pub cwd: String,
    /// Extra environment variables for the spawned subprocess, each
    /// `"KEY=VALUE"` ([`parse_agent_env`]). Entries with no `=` are
    /// skipped rather than erroring.
    #[serde(default)]
    pub env: Vec<String>,
}

/// Parse an [`AcpAgentProfile::env`] list into `(key, value)` pairs for
/// [`AcpClient::spawn_with_env`]. Entries with no `=` are silently skipped
/// — a config typo degrades quietly, same posture as
/// [`parse_agent_command`]'s empty-token handling.
pub fn parse_agent_env(env: &[String]) -> Vec<(String, String)> {
    env.iter()
        .filter_map(|entry| entry.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.to_string()))
        .collect()
}

/// Which kind of `session/update` chunk a notification carries, as mapped
/// onto the AI panel's own transcript roles by
/// `Engine::acp_append_chunk` (`src/core/engine/acp_ops.rs`, ACP-1,
/// #952). Kept here rather than in the engine because recognizing the
/// wire shape (`sessionUpdate` tag + `content.text`) is transport-schema
/// knowledge, not panel policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpChunkKind {
    /// `agent_message_chunk` — the assistant's visible reply text.
    Message,
    /// `agent_thought_chunk` — the assistant's internal reasoning.
    /// Rendered under a visually distinct role from `Message` per ACP-1's
    /// acceptance criteria (do not merge the two).
    Thought,
    /// `user_message_chunk` — the agent echoing back user-authored
    /// content (e.g. from another client sharing the same session).
    UserEcho,
}

/// Extract `(kind, text)` from a `session/update` notification's `update`
/// object, per the ACP v1 schema: a `sessionUpdate` string tag plus a
/// `content` [`ContentBlock`](https://agentclientprotocol.com/protocol/content)
/// (`{"type": "text", "text": "..."}`).
///
/// Returns `None` for update kinds this slice does not render —
/// `tool_call`/`tool_call_update`/`plan`, left to ACP-4/ACP-5 per #952's
/// scope — or for a malformed/unrecognized shape. Never panics; a missing
/// `content.text` yields an empty string rather than dropping the chunk,
/// so a still-loading tool-adjacent update doesn't desync the stream.
pub fn session_update_chunk(update: &serde_json::Value) -> Option<(AcpChunkKind, String)> {
    let kind = update.get("sessionUpdate").and_then(|v| v.as_str())?;
    let chunk_kind = match kind {
        "agent_message_chunk" => AcpChunkKind::Message,
        "agent_thought_chunk" => AcpChunkKind::Thought,
        "user_message_chunk" => AcpChunkKind::UserEcho,
        _ => return None,
    };
    let text = update
        .get("content")
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .to_string();
    Some((chunk_kind, text))
}

// ---------------------------------------------------------------------------
// plan, available_commands_update, current_mode_update, usage_update
// (#956, ACP-5)
// ---------------------------------------------------------------------------

/// The plan model itself is source-agnostic and lives in
/// [`crate::core::plan`] (#529, Track A Phase 4) — a future Board/remote-
/// worker plan preview reuses the exact same [`PlanEntry`]/
/// [`PlanEntryStatus`]/`plan_to_checklist_text` with a completely
/// different feeder, so nothing ACP-specific belongs in the model itself.
/// Re-exported here under their original names so every existing ACP call
/// site (`Engine::acp_plan`, `render::populate_ai_chat_controller`) is
/// unaffected by the move.
pub use crate::core::plan::{
    plan_to_checklist_text, PlanEntry as AcpPlanEntry, PlanEntryStatus as AcpPlanEntryStatus,
};

/// Parse a `session/update`'s `plan` variant: `{"sessionUpdate": "plan",
/// "entries": [{"content", "status"}]}`. Returns `None` for a non-`plan`
/// update. The envelope check (`sessionUpdate == "plan"`) is ACP's own —
/// entry parsing itself is [`crate::core::plan::parse_plan_entries`], which
/// knows nothing about this envelope and is reused as-is by any other
/// plan-preview producer.
///
/// **Every call is a full replacement, never a delta** — see
/// `Engine::acp_plan`'s doc. Treating consecutive `plan` updates as
/// append-only is the single most common way to get this variant wrong
/// per #956's own scope note; nothing in this function accumulates state,
/// by construction, since it takes no previous plan as input.
pub fn parse_plan_update(update: &serde_json::Value) -> Option<Vec<AcpPlanEntry>> {
    if update.get("sessionUpdate").and_then(|v| v.as_str()) != Some("plan") {
        return None;
    }
    crate::core::plan::parse_plan_entries(update)
}

/// One slash command the agent declared via `available_commands_update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAvailableCommand {
    pub name: String,
    pub description: String,
}

/// Parse a `session/update`'s `available_commands_update` variant:
/// `{"sessionUpdate": "available_commands_update", "availableCommands":
/// [{"name", "description"}]}`. Like `plan`, this is a full replacement of
/// whatever command set was known before — see `Engine::
/// acp_available_commands`. An entry missing `name` is dropped (nothing a
/// user could usefully type); a missing `description` defaults to `""`.
pub fn parse_available_commands_update(
    update: &serde_json::Value,
) -> Option<Vec<AcpAvailableCommand>> {
    if update.get("sessionUpdate").and_then(|v| v.as_str()) != Some("available_commands_update") {
        return None;
    }
    let commands = update.get("availableCommands").and_then(|v| v.as_array())?;
    Some(
        commands
            .iter()
            .filter_map(|c| {
                let name = c.get("name")?.as_str()?.to_string();
                let description = c
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                Some(AcpAvailableCommand { name, description })
            })
            .collect(),
    )
}

/// One mode an agent exposes (`session/new`'s `modes.availableModes`).
/// `current_mode_update` only ever changes *which* mode id is current —
/// this list itself is only ever supplied at the handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpSessionMode {
    pub id: String,
    pub name: String,
}

/// Parse `session/new`'s `modes` result field: `{"currentModeId",
/// "availableModes": [{"id", "name"}]}`. Returns `(current_mode_id,
/// modes)` — an absent/malformed `modes` value (an agent that doesn't
/// support modes at all) yields `(None, vec![])`, not an error; modes are
/// an optional agent capability, not a required one. A mode entry missing
/// `name` falls back to its `id` (still selectable/displayable, just less
/// friendly), matching this module's general "never silently drop
/// something a user could otherwise act on" policy.
pub fn parse_session_modes(modes: &serde_json::Value) -> (Option<String>, Vec<AcpSessionMode>) {
    let current = modes
        .get("currentModeId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let list = modes
        .get("availableModes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m.get("id")?.as_str()?.to_string();
                    let name = m
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&id)
                        .to_string();
                    Some(AcpSessionMode { id, name })
                })
                .collect()
        })
        .unwrap_or_default();
    (current, list)
}

/// Parse a `session/update`'s `current_mode_update` variant:
/// `{"sessionUpdate": "current_mode_update", "currentModeId": "..."}`.
/// This is the **only** thing that should ever change `Engine::
/// acp_current_mode_id` — a `session/set_mode` request succeeding is not
/// itself sufficient, per #956's acceptance bar ("the displayed mode
/// follows `current_mode_update`").
pub fn parse_current_mode_update(update: &serde_json::Value) -> Option<String> {
    if update.get("sessionUpdate").and_then(|v| v.as_str()) != Some("current_mode_update") {
        return None;
    }
    update
        .get("currentModeId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Token/cost telemetry from a `session/update`'s `usage_update` variant.
/// Every field is independently optional — an agent may report only
/// tokens, only cost, or both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcpUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_cost_usd: Option<f64>,
}

/// Parse a `session/update`'s `usage_update` variant. Tolerant of the
/// fields living either nested under a `usage` object or flattened onto
/// `update` directly, and of either `inputTokens`/`outputTokens` or
/// `promptTokens`/`completionTokens` naming — token/cost telemetry is a
/// newer, less-stable corner of the ACP v1 schema than the rest of this
/// module, so this reads defensively rather than committing to one exact
/// shape. Returns `None` when the tag doesn't match, or when it matches
/// but *no* recognizable field is present at all (never for a
/// partially-populated report — a cost-only or tokens-only update still
/// renders something).
pub fn parse_usage_update(update: &serde_json::Value) -> Option<AcpUsage> {
    if update.get("sessionUpdate").and_then(|v| v.as_str()) != Some("usage_update") {
        return None;
    }
    let usage = update.get("usage").unwrap_or(update);
    let input_tokens = usage
        .get("inputTokens")
        .or_else(|| usage.get("promptTokens"))
        .and_then(|v| v.as_u64());
    let output_tokens = usage
        .get("outputTokens")
        .or_else(|| usage.get("completionTokens"))
        .and_then(|v| v.as_u64());
    let total_cost_usd = usage
        .get("totalCostUsd")
        .or_else(|| usage.get("costUsd"))
        .or_else(|| usage.get("cost"))
        .and_then(|v| v.as_f64());
    if input_tokens.is_none() && output_tokens.is_none() && total_cost_usd.is_none() {
        return None;
    }
    Some(AcpUsage {
        input_tokens,
        output_tokens,
        total_cost_usd,
    })
}

/// Format an [`AcpUsage`] for the AI panel's status header — compact, and
/// only the fields actually present (never e.g. `"0 tok · $0.0000"` for a
/// report that only carried cost).
pub fn format_usage_summary(usage: &AcpUsage) -> String {
    let mut parts = Vec::new();
    match (usage.input_tokens, usage.output_tokens) {
        (Some(i), Some(o)) => parts.push(format!("{i}+{o} tok")),
        (Some(i), None) => parts.push(format!("{i} tok in")),
        (None, Some(o)) => parts.push(format!("{o} tok out")),
        (None, None) => {}
    }
    if let Some(cost) = usage.total_cost_usd {
        parts.push(format!("${cost:.4}"));
    }
    parts.join(" \u{b7} ")
}

// ---------------------------------------------------------------------------
// fs/read_text_file, fs/write_text_file — wire-shape parsing + path safety
// (#954, ACP-3)
// ---------------------------------------------------------------------------

/// A parsed `fs/read_text_file` request: `{sessionId, path, line?, limit?}`
/// per the ACP v1 schema. `line` is 1-based (matching the wire), `limit` is
/// a line count — see [`select_text_lines`] for how the two combine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadTextFileParams {
    pub session_id: String,
    pub path: String,
    pub line: Option<u32>,
    pub limit: Option<u32>,
}

/// Parse a `fs/read_text_file` request's `params`. Returns `None` for a
/// malformed request (missing `sessionId` or `path`) — the caller must still
/// answer such a request with a JSON-RPC error, never silence.
pub fn parse_read_text_file_params(params: &serde_json::Value) -> Option<ReadTextFileParams> {
    let session_id = params.get("sessionId")?.as_str()?.to_string();
    let path = params.get("path")?.as_str()?.to_string();
    let line = params
        .get("line")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);
    Some(ReadTextFileParams {
        session_id,
        path,
        line,
        limit,
    })
}

/// Build the `result` value for a `fs/read_text_file` reply: `{content}`.
pub fn read_text_file_result(content: &str) -> serde_json::Value {
    serde_json::json!({"content": content})
}

/// Apply `fs/read_text_file`'s optional `line` (1-based start line) /
/// `limit` (max number of lines to return) to a file's full text content.
/// Missing `line` starts at the beginning; missing `limit` returns
/// everything from `line` onward; both missing returns `content` unchanged.
/// Never panics on an out-of-range `line`/`limit` — clamps instead of
/// indexing past the end.
pub fn select_text_lines(content: &str, line: Option<u32>, limit: Option<u32>) -> String {
    if line.is_none() && limit.is_none() {
        return content.to_string();
    }
    let lines: Vec<&str> = content.split('\n').collect();
    let start = line
        .map(|l| l.saturating_sub(1) as usize)
        .unwrap_or(0)
        .min(lines.len());
    let end = match limit {
        Some(n) => start.saturating_add(n as usize).min(lines.len()),
        None => lines.len(),
    };
    lines[start..end].join("\n")
}

/// A parsed `fs/write_text_file` request: `{sessionId, path, content}` per
/// the ACP v1 schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteTextFileParams {
    pub session_id: String,
    pub path: String,
    pub content: String,
}

/// Parse a `fs/write_text_file` request's `params`. Returns `None` for a
/// malformed request (missing `sessionId`, `path`, or `content`).
pub fn parse_write_text_file_params(params: &serde_json::Value) -> Option<WriteTextFileParams> {
    let session_id = params.get("sessionId")?.as_str()?.to_string();
    let path = params.get("path")?.as_str()?.to_string();
    let content = params.get("content")?.as_str()?.to_string();
    Some(WriteTextFileParams {
        session_id,
        path,
        content,
    })
}

/// Resolve `..`/`.` components purely lexically (no filesystem access) —
/// the same technique `path-clean`-style crates use. This is the first half
/// of [`resolve_path_within_roots`]'s traversal defence: a request for
/// `<cwd>/../../etc/passwd` must not pass a `starts_with(cwd)` check just
/// because the string happens to start with the right prefix before the
/// `..` components are accounted for.
fn lexically_normalize(path: &Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve `path` (absolute, or joined onto `roots[0]` if relative — ACP
/// paths are supposed to be absolute already, but this is defensive rather
/// than a panic) and confirm it falls inside one of `roots` (the session
/// `cwd`, plus any future `additionalDirectories` — see
/// `Engine::acp_workspace_roots`'s doc for why that list is one entry today).
///
/// Two traversal vectors are guarded against: lexical `..` components (via
/// [`lexically_normalize`]) and a symlink inside an allowed root pointing
/// outside it (by canonicalizing the nearest existing ancestor and
/// re-attaching whatever suffix doesn't exist on disk yet — the target of a
/// `fs/write_text_file` call creating a brand new file, most commonly).
///
/// Returns the resolved, safe-to-use path on success, or an error message
/// naming the offending path on failure — never silently narrows to "no" by
/// returning a root instead of erroring.
pub fn resolve_path_within_roots(
    path: &Path,
    roots: &[std::path::PathBuf],
) -> Result<std::path::PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        roots
            .first()
            .cloned()
            .unwrap_or_else(|| std::path::PathBuf::from("/"))
            .join(path)
    };
    let normalized = lexically_normalize(&absolute);

    let mut base = normalized.clone();
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    while !base.exists() {
        match base.file_name().map(|n| n.to_os_string()) {
            Some(name) => {
                suffix.push(name);
                if !base.pop() {
                    break;
                }
            }
            None => break,
        }
    }
    let canonical_base = base
        .canonicalize()
        .map_err(|e| format!("cannot resolve {}: {e}", normalized.display()))?;
    let mut resolved = canonical_base;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }

    let allowed = roots.iter().any(|root| {
        root.canonicalize()
            .map(|canonical_root| resolved.starts_with(&canonical_root))
            .unwrap_or(false)
    });
    if allowed {
        Ok(resolved)
    } else {
        Err(format!(
            "{} is outside the session workspace",
            resolved.display()
        ))
    }
}

// ---------------------------------------------------------------------------
// authMethods — wire-shape parsing (#957, ACP-6)
// ---------------------------------------------------------------------------

/// Which RPC flow authenticates an [`AcpAuthMethod`] — the distinction the
/// issue calls out as "the trap": `Agent` methods answer via the plain
/// `authenticate {methodId}` request ([`AcpClient::authenticate`]); a
/// `Terminal` method does **not** call `authenticate` at all — the client
/// must run the agent's own command line attached to a real TTY so its CLI
/// can drive an interactive login, then re-initialize once that process
/// exits (see `Engine::acp_launch_terminal_login` /
/// `Engine::acp_finish_terminal_login`, `src/core/engine/terminal_ops.rs`
/// and `acp_ops.rs`).
///
/// An unrecognized or missing `type` on the wire defaults to `Agent`:
/// `authenticate` is the RPC every ACP v1 agent already speaks (it predates
/// `auth.terminal`), so defaulting there means this client can still
/// attempt one ordinary JSON-RPC call and surface a normal
/// [`AcpEvent::RequestFailed`] on failure — never silently launching an
/// interactive subprocess because a `type` value this client doesn't
/// recognize came back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpAuthMethodKind {
    Agent,
    Terminal,
}

/// One entry from `initialize`'s `authMethods` array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAuthMethod {
    pub id: String,
    /// Display name shown on the auth-choice dialog button. Falls back to
    /// `id` when the agent omits it — still selectable, just less
    /// friendly, matching this module's general "never silently drop
    /// something a user could otherwise act on" policy (see
    /// `parse_request_permission`'s doc for the same call elsewhere).
    pub name: String,
    pub kind: AcpAuthMethodKind,
}

/// Parse `initialize`'s `authMethods` result field: an array of `{id, name,
/// type}`. An entry missing `id` is dropped (nothing a user could
/// meaningfully select or key `authenticate` on); every other field is
/// defaulted rather than dropping the whole entry — see [`AcpAuthMethod::
/// name`] and [`AcpAuthMethodKind`]'s docs for the specific defaults and why.
pub fn parse_auth_methods(methods: &[serde_json::Value]) -> Vec<AcpAuthMethod> {
    methods
        .iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            let name = m
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&id)
                .to_string();
            let kind = match m.get("type").and_then(|v| v.as_str()) {
                Some("terminal") => AcpAuthMethodKind::Terminal,
                _ => AcpAuthMethodKind::Agent,
            };
            Some(AcpAuthMethod { id, name, kind })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// session/request_permission — wire-shape parsing (#953, ACP-2)
// ---------------------------------------------------------------------------

/// One option the agent offered in a `session/request_permission` request.
/// `kind` is one of `allow_once` / `allow_always` / `reject_once` /
/// `reject_always` per the ACP v1 schema — the *option's* kind, not
/// [`AcpToolCallInfo::kind`] (the tool-call's category, e.g. `"edit"`).
/// Kept as a raw `String` rather than an enum: an agent sending a kind this
/// client doesn't recognize should still render as a selectable button
/// (whatever `name` says) rather than silently vanishing. A missing `kind`
/// on the wire parses to `""` (see `parse_request_permission`), not
/// `"allow_once"` — never default toward the most permissive category on
/// missing data in a permission-approval path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPermissionOption {
    pub option_id: String,
    pub name: String,
    pub kind: String,
}

/// The `toolCall` a `session/request_permission` request asks about, pared
/// down to exactly what #953's acceptance bar requires a human see before
/// deciding: `title`, `kind`, `locations`. Everything else `toolCall` may
/// carry (`rawInput`, `content`, `status`, ...) is out of scope for this
/// slice's dialog (ACP-4/5 render the fuller tool-call shape elsewhere).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpToolCallInfo {
    pub title: String,
    /// The tool-call's category (e.g. `"edit"`, `"execute"`, `"read"`) —
    /// coarser than a specific tool identity, and deliberately so: it is
    /// what [`Engine::acp_remembered_decisions`](crate::core::engine::Engine)
    /// keys `allow_always`/`reject_always` on, so "same tool" reads as
    /// "same category of action" rather than e.g. "same file path".
    pub kind: String,
    /// `(path, line)` — `line` is 1-based and `None` when the agent didn't
    /// supply one.
    pub locations: Vec<(String, Option<u32>)>,
}

/// A parsed `session/request_permission` request — the whole payload the
/// permission dialog needs, independent of the JSON-RPC `id` (the caller,
/// [`crate::core::engine::Engine::poll_acp`], already has that from
/// [`AcpEvent::ClientRequest`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPermissionRequest {
    pub session_id: String,
    pub tool_call: AcpToolCallInfo,
    pub options: Vec<AcpPermissionOption>,
}

/// Parse a `session/request_permission` request's `params` object per the
/// ACP v1 schema: `{sessionId, toolCall: {title, kind, locations}, options:
/// [{optionId, name, kind}]}`. Returns `None` for a malformed request
/// (missing `sessionId`, no `options` array, or an option missing
/// `optionId`) — the caller must still answer such a request (with a
/// JSON-RPC error, not silence) rather than open a dialog with nothing
/// selectable in it.
pub fn parse_request_permission(params: &serde_json::Value) -> Option<AcpPermissionRequest> {
    let session_id = params.get("sessionId")?.as_str()?.to_string();
    let tool_call_json = params.get("toolCall")?;
    let title = tool_call_json
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Tool call")
        .to_string();
    let kind = tool_call_json
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("other")
        .to_string();
    let locations = tool_call_json
        .get("locations")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|loc| {
                    let path = loc.get("path")?.as_str()?.to_string();
                    let line = loc.get("line").and_then(|v| v.as_u64()).map(|n| n as u32);
                    Some((path, line))
                })
                .collect()
        })
        .unwrap_or_default();

    let options_json = params.get("options")?.as_array()?;
    let mut options = Vec::with_capacity(options_json.len());
    for opt in options_json {
        let option_id = opt.get("optionId")?.as_str()?.to_string();
        let name = opt
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Option")
            .to_string();
        // A spec-conformant agent always sends `kind`; a missing one is
        // malformed input, not a signal to default toward the *most*
        // permissive category. Default to `""` (matches neither the
        // `allow_`/`reject_` prefix `Engine::acp_handle_permission_request`
        // checks for a remembered `allow_always`/`reject_always` decision,
        // nor anything a real agent would send) so the option still renders
        // as a selectable button — never silently vanishes — but can never
        // be auto-answered from memory and can never masquerade as
        // `allow_once` (#953 review: "defaulting toward allow on missing
        // data in a permission-approval path is the wrong direction").
        let opt_kind = opt
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        options.push(AcpPermissionOption {
            option_id,
            name,
            kind: opt_kind,
        });
    }
    if options.is_empty() {
        return None;
    }

    Some(AcpPermissionRequest {
        session_id,
        tool_call: AcpToolCallInfo {
            title,
            kind,
            locations,
        },
        options,
    })
}

/// Build the `result` value for a `session/request_permission` reply where
/// the human picked `option_id`. Shape: `{outcome: {outcome: "selected",
/// optionId}}` per the ACP v1 schema.
pub fn permission_outcome_selected(option_id: &str) -> serde_json::Value {
    serde_json::json!({"outcome": {"outcome": "selected", "optionId": option_id}})
}

/// Build the `result` value for a `session/request_permission` reply when
/// no option was chosen (Esc-dismissed, the session was cancelled, an
/// unrelated dialog replaced it, ...). Shape: `{outcome: {outcome:
/// "cancelled"}}` per the ACP v1 schema. This is the reply every parked
/// permission request must eventually get exactly one of, per #953's
/// hang-avoidance requirement — see `Engine::acp_cancel_pending_permission`.
pub fn permission_outcome_cancelled() -> serde_json::Value {
    serde_json::json!({"outcome": {"outcome": "cancelled"}})
}

// ---------------------------------------------------------------------------
// tool_call / tool_call_update (#955, ACP-4)
// ---------------------------------------------------------------------------

/// Lifecycle status of an [`AcpToolCall`], per the ACP v1 `tool_call`/
/// `tool_call_update` `status` field. Unknown/missing defaults to
/// [`Self::Pending`] — same "never default toward done on malformed
/// input" policy as [`AcpPlanEntryStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpToolCallStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

impl AcpToolCallStatus {
    fn parse(s: Option<&str>) -> Self {
        match s {
            Some("in_progress") => Self::InProgress,
            Some("completed") => Self::Completed,
            Some("failed") => Self::Failed,
            _ => Self::Pending,
        }
    }

    /// Bracketed glyph used as the transcript summary line's prefix, e.g.
    /// `[~] edit: Edit src/main.rs` — visually distinct at every stage of
    /// `pending -> in_progress -> completed | failed` without needing
    /// colour (both TUI and GTK render this the same plain text).
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Pending => "[ ]",
            Self::InProgress => "[~]",
            Self::Completed => "[x]",
            Self::Failed => "[!]",
        }
    }
}

/// One block of an [`AcpToolCall::content`] list, per the ACP v1 schema's
/// tagged union. `{type: "content"}` (text/image output) — only the text
/// case renders; image content parses to `None` rather than an
/// empty-string block, so it's visibly absent instead of a confusing
/// blank line. `{type: "diff", path, oldText, newText}` is the shape
/// #955 exists for — it opens the shared change-review surface
/// (`crate::core::review`). `{type: "terminal"}` is out of scope for the
/// whole ACP track (see this module's top doc, "Skip `terminal/*`") but
/// still parses to a variant (rather than being dropped) so a tool call
/// that is entirely terminal content still round-trips as "has content".
#[derive(Debug, Clone, PartialEq)]
pub enum AcpToolCallContentBlock {
    Text(String),
    Diff {
        path: String,
        old_text: Option<String>,
        new_text: String,
    },
    Terminal,
}

fn parse_tool_call_content_block(block: &serde_json::Value) -> Option<AcpToolCallContentBlock> {
    match block.get("type").and_then(|v| v.as_str())? {
        "content" => {
            let text = block.get("content")?.get("text")?.as_str()?.to_string();
            Some(AcpToolCallContentBlock::Text(text))
        }
        "diff" => {
            let path = block.get("path")?.as_str()?.to_string();
            let new_text = block.get("newText")?.as_str()?.to_string();
            let old_text = block
                .get("oldText")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            Some(AcpToolCallContentBlock::Diff {
                path,
                old_text,
                new_text,
            })
        }
        "terminal" => Some(AcpToolCallContentBlock::Terminal),
        _ => None,
    }
}

fn parse_tool_call_content(update: &serde_json::Value) -> Vec<AcpToolCallContentBlock> {
    update
        .get("content")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(parse_tool_call_content_block)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_tool_call_locations(update: &serde_json::Value) -> Vec<(String, Option<u32>)> {
    update
        .get("locations")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|loc| {
                    let path = loc.get("path")?.as_str()?.to_string();
                    let line = loc.get("line").and_then(|v| v.as_u64()).map(|n| n as u32);
                    Some((path, line))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One tool call the agent is executing or has executed — the addressable
/// unit `tool_call`/`tool_call_update` operate on, keyed by `id`
/// (`toolCallId` on the wire). Stored as an upserted `Vec` — not an
/// append-only log — by `Engine::acp_tool_calls`.
#[derive(Debug, Clone, PartialEq)]
pub struct AcpToolCall {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: AcpToolCallStatus,
    pub locations: Vec<(String, Option<u32>)>,
    pub content: Vec<AcpToolCallContentBlock>,
}

/// Parse a `session/update`'s `tool_call` variant — the tool call's
/// initial announcement. Returns `None` for a non-`tool_call` update or
/// one missing `toolCallId`/`title` (the two the issue marks required).
/// `kind` defaults to `"other"`, `status` to `Pending` — same "never
/// default toward done/allowed on missing data" policy used throughout
/// this module.
pub fn parse_tool_call(update: &serde_json::Value) -> Option<AcpToolCall> {
    if update.get("sessionUpdate").and_then(|v| v.as_str()) != Some("tool_call") {
        return None;
    }
    let id = update.get("toolCallId")?.as_str()?.to_string();
    let title = update.get("title")?.as_str()?.to_string();
    let kind = update
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("other")
        .to_string();
    let status = AcpToolCallStatus::parse(update.get("status").and_then(|v| v.as_str()));
    Some(AcpToolCall {
        id,
        title,
        kind,
        status,
        locations: parse_tool_call_locations(update),
        content: parse_tool_call_content(update),
    })
}

/// A parsed `tool_call_update` — a patch against an existing
/// [`AcpToolCall`] by `id`, never a wholesale replacement. `status: None`
/// means the update didn't touch status; `content: None` means the wire
/// message had no `content` field at all. A present `content` is
/// **appended** to the existing call's content by the caller (never
/// replaced) — this issue's own framing: "status transitions and
/// appended content".
#[derive(Debug, Clone, PartialEq)]
pub struct AcpToolCallUpdate {
    pub id: String,
    pub status: Option<AcpToolCallStatus>,
    pub content: Option<Vec<AcpToolCallContentBlock>>,
}

/// Parse a `session/update`'s `tool_call_update` variant. Returns `None`
/// for a non-`tool_call_update` update or one missing `toolCallId`.
pub fn parse_tool_call_update(update: &serde_json::Value) -> Option<AcpToolCallUpdate> {
    if update.get("sessionUpdate").and_then(|v| v.as_str()) != Some("tool_call_update") {
        return None;
    }
    let id = update.get("toolCallId")?.as_str()?.to_string();
    let status = update
        .get("status")
        .and_then(|v| v.as_str())
        .map(|s| AcpToolCallStatus::parse(Some(s)));
    let content = update.get("content").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(parse_tool_call_content_block)
            .collect()
    });
    Some(AcpToolCallUpdate {
        id,
        status,
        content,
    })
}

/// Render one [`AcpToolCall`] as its collapsed transcript summary line —
/// `title` + `kind` per the issue's acceptance bar, prefixed by
/// [`AcpToolCallStatus::glyph`] so `pending -> in_progress -> completed |
/// failed` transitions are visible without expanding anything, plus one
/// indented `-> path[:line]` line per location. Content blocks
/// (`Text`/`Terminal`) are never inlined here — "collapsed by default" —
/// a `Diff` block instead opens the change-review surface
/// (`crate::core::engine::review_ops::acp_open_review_for_new_diffs`)
/// rather than dumping a patch into the chat log.
pub fn tool_call_summary_line(call: &AcpToolCall) -> String {
    let mut line = format!("{} {}: {}", call.status.glyph(), call.kind, call.title);
    for (path, ln) in &call.locations {
        match ln {
            Some(l) => line.push_str(&format!("\n    \u{2192} {path}:{l}")),
            None => line.push_str(&format!("\n    \u{2192} {path}")),
        }
    }
    line
}

// ---------------------------------------------------------------------------
// AcpClient — owns one agent subprocess and its session lifecycle
// ---------------------------------------------------------------------------

pub struct AcpClient {
    stdin: Arc<Mutex<Box<dyn IoWrite + Send>>>,
    next_request_id: i64,
    child: Child,
    /// Our client -> agent requests awaiting a response: id -> method.
    pending_requests: Arc<Mutex<HashMap<i64, String>>>,
    /// Agent -> client requests we have not yet answered: id -> method.
    /// Populated by the reader thread when an `AcpEvent::ClientRequest` is
    /// emitted; drained by [`Self::respond_to_client_request`].
    pending_client_requests: Arc<Mutex<HashMap<i64, String>>>,
    rx: mpsc::Receiver<AcpEvent>,
    // Held only to keep the thread alive; dropped (and thus joined-on-exit
    // implicitly via detach) when AcpClient is dropped.
    _reader_thread: Option<thread::JoinHandle<()>>,
}

impl AcpClient {
    /// Spawn an agent subprocess from `argv` (argv[0] is the program) with
    /// `cwd` as its working directory.
    pub fn spawn(argv: &[String], cwd: &Path) -> Result<Self, String> {
        Self::spawn_with_env(argv, cwd, &[])
    }

    /// Like [`Self::spawn`], but with extra environment variables set on the
    /// child. Exists mainly so tests can drive the fake agent fixture into
    /// specific scripted branches (e.g. "die right after initialize")
    /// without needing a family of near-identical fixture scripts.
    pub fn spawn_with_env(
        argv: &[String],
        cwd: &Path,
        extra_env: &[(&str, &str)],
    ) -> Result<Self, String> {
        let (program, args) = argv.split_first().ok_or("empty ACP agent command")?;

        let mut cmd = crate::core::git::hidden_command_new_process_group(program);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start ACP agent {program}: {e}"))?;

        let stdout = child.stdout.take().ok_or("Failed to get agent stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to get agent stderr")?;
        let stdin: Box<dyn IoWrite + Send> =
            Box::new(child.stdin.take().ok_or("Failed to get agent stdin")?);
        let stdin = Arc::new(Mutex::new(stdin));

        let pending_requests: Arc<Mutex<HashMap<i64, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_client_requests: Arc<Mutex<HashMap<i64, String>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Stderr is free-form agent log noise per the ACP transport spec —
        // never protocol. Collect a bounded ring for crash diagnostics,
        // same shape as `LspServer::start`.
        let stderr_buf: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let stderr_buf_clone = stderr_buf.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if let Ok(mut buf) = stderr_buf_clone.lock() {
                            if buf.len() < 2048 {
                                if !buf.is_empty() {
                                    buf.push('\n');
                                }
                                buf.push_str(&l);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let (tx, rx) = mpsc::channel();
        let reader_pending = pending_requests.clone();
        let reader_pending_client = pending_client_requests.clone();
        let reader_stdin = stdin.clone();
        let reader_thread = thread::spawn(move || {
            reader_thread_main(
                stdout,
                tx,
                reader_pending,
                reader_pending_client,
                reader_stdin,
                stderr_buf,
            );
        });

        Ok(Self {
            stdin,
            next_request_id: 1,
            child,
            pending_requests,
            pending_client_requests,
            rx,
            _reader_thread: Some(reader_thread),
        })
    }

    fn send_request(&mut self, method: &str, params: serde_json::Value) -> i64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        if let Ok(mut pending) = self.pending_requests.lock() {
            pending.insert(id, method.to_string());
        }
        self.send_raw(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        id
    }

    fn send_notification(&self, method: &str, params: serde_json::Value) {
        self.send_raw(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    fn send_raw(&self, body: &serde_json::Value) {
        let encoded = encode_ndjson_line(body);
        if let Ok(mut stdin) = self.stdin.lock() {
            let _ = stdin.write_all(&encoded);
            let _ = stdin.flush();
        }
    }

    /// Send the `initialize` request. Advertises `fs.readTextFile` and
    /// `fs.writeTextFile` (#954, ACP-3) and `auth.terminal` (#957, ACP-6) —
    /// the only `clientCapabilities` this client implements so far, per the
    /// module doc's capability negotiation commitment ("omitted means
    /// unsupported"). `auth.terminal: true` is what makes an agent's
    /// `initialize` response allowed to offer a `type: "terminal"` auth
    /// method at all (e.g. `claude-ai-login` / "Claude Subscription" from
    /// `@agentclientprotocol/claude-agent-acp`) — omitting it is how a
    /// client opts out of subscription login and forces API-key-only, which
    /// this client deliberately does not do (see the module doc's ACP-6
    /// section). `terminal/*` — the unrelated *client-served* terminal
    /// methods — is still never advertised (out of scope for the whole
    /// track).
    pub fn initialize(&mut self) -> i64 {
        self.send_request(
            "initialize",
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "clientCapabilities": {
                    "fs": {
                        "readTextFile": true,
                        "writeTextFile": true,
                    },
                    "auth": {
                        "terminal": true,
                    },
                },
                "clientInfo": {
                    "name": "vimcode",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )
    }

    /// Send an `authenticate` request for a `type: "agent"` auth method
    /// (#957, ACP-6) — `{"methodId": method_id}`. Distinct from `type:
    /// "terminal"` methods, which never call this RPC at all; see the
    /// module doc's "How terminal auth differs from `authenticate`" note
    /// and [`AcpAuthMethodKind`].
    pub fn authenticate(&mut self, method_id: &str) -> i64 {
        self.send_request("authenticate", serde_json::json!({"methodId": method_id}))
    }

    /// Send `session/new`. `cwd` must be absolute per the ACP spec; this
    /// canonicalizes it for the caller.
    pub fn new_session(&mut self, cwd: &Path, mcp_servers: Vec<serde_json::Value>) -> i64 {
        self.send_request(
            "session/new",
            serde_json::json!({
                "cwd": absolute_path_string(cwd),
                "mcpServers": mcp_servers,
            }),
        )
    }

    /// Send `session/prompt`. `prompt` is the ACP content-block array.
    pub fn prompt(&mut self, session_id: &str, prompt: Vec<serde_json::Value>) -> i64 {
        self.send_request(
            "session/prompt",
            serde_json::json!({
                "sessionId": session_id,
                "prompt": prompt,
            }),
        )
    }

    /// Send `session/set_mode` (#956, ACP-5). The displayed mode does not
    /// change from this call's response — only from the agent's own
    /// `current_mode_update` notification afterward; see
    /// `Engine::acp_set_mode`'s doc for why.
    pub fn set_mode(&mut self, session_id: &str, mode_id: &str) -> i64 {
        self.send_request(
            "session/set_mode",
            serde_json::json!({"sessionId": session_id, "modeId": mode_id}),
        )
    }

    /// Send `session/cancel` (a notification — no response expected).
    pub fn cancel(&self, session_id: &str) {
        self.send_notification(
            "session/cancel",
            serde_json::json!({ "sessionId": session_id }),
        );
    }

    /// Answer a parked agent -> client request. Safe to call from a
    /// completely different call site/time than where the
    /// `AcpEvent::ClientRequest` was received — the reply is written
    /// through the same shared `stdin` the reader thread holds, so it
    /// reaches the agent regardless of when it's called.
    pub fn respond_to_client_request(
        &self,
        request_id: i64,
        result: Result<serde_json::Value, (i64, String)>,
    ) {
        if let Ok(mut pending) = self.pending_client_requests.lock() {
            pending.remove(&request_id);
        }
        let msg = match result {
            Ok(value) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": value,
            }),
            Err((code, message)) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "error": { "code": code, "message": message },
            }),
        };
        self.send_raw(&msg);
    }

    /// Non-blocking drain of agent events, capped at `MAX_EVENTS_PER_POLL`
    /// per call so a burst of `session/update` chunks can't stall a tick —
    /// same cap shape as `LspManager::poll_events`.
    pub fn poll(&mut self) -> Vec<AcpEvent> {
        let mut events = Vec::new();
        while events.len() < MAX_EVENTS_PER_POLL {
            match self.rx.try_recv() {
                Ok(event) => events.push(event),
                Err(_) => break,
            }
        }
        events
    }

    /// Request ids the agent has issued to us that we have not yet answered.
    /// Exposed for tests/diagnostics — the source of truth engine code
    /// should use is the `AcpEvent::ClientRequest` stream, not this.
    #[allow(dead_code)] // exercised by tests; will back a future "pending permission" UI slice
    pub fn pending_client_request_count(&self) -> usize {
        self.pending_client_requests
            .lock()
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Whether the child process has exited (non-blocking check).
    pub fn has_exited(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        // Best-effort: don't leave the agent process running after we stop
        // tracking it. Closing stdin (dropping the Arc's last strong ref
        // isn't guaranteed here since the reader thread also holds a
        // clone) would be a politer shutdown for a well-behaved agent, but
        // an explicit kill is the only thing that's guaranteed to work
        // against a wedged one — and unlike `LspServer`/`DapServer`, ACP
        // has no adapter-agnostic "please exit" request to send first.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Reader thread
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn reader_thread_main(
    stdout: impl IoRead + Send + 'static,
    tx: Sender<AcpEvent>,
    pending_requests: Arc<Mutex<HashMap<i64, String>>>,
    pending_client_requests: Arc<Mutex<HashMap<i64, String>>>,
    stdin: Arc<Mutex<Box<dyn IoWrite + Send>>>,
    stderr_buf: Arc<Mutex<String>>,
) {
    // `stdin` is threaded through only so a future slice can auto-answer
    // agent requests it recognizes inline (the "handler returns
    // immediately" half of the dispatch contract described in the module
    // doc); ACP-0 always parks by emitting `ClientRequest` and lets the
    // caller answer via `AcpClient::respond_to_client_request`.
    let _ = &stdin;

    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut was_initialized = false;

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF — agent exited
            Ok(_) => {}
            Err(_) => break,
        }

        match classify_line(&line) {
            ParsedLine::Unusable => continue, // malformed/blank — resync on next line
            ParsedLine::AgentToClientRequest { id, method, params } => {
                if let Ok(mut pending) = pending_client_requests.lock() {
                    pending.insert(id, method.clone());
                }
                if tx
                    .send(AcpEvent::ClientRequest {
                        request_id: id,
                        method,
                        params,
                    })
                    .is_err()
                {
                    break; // receiver gone
                }
            }
            ParsedLine::Notification { method, params } => {
                if method == "session/update" {
                    let session_id = params
                        .get("sessionId")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    if tx
                        .send(AcpEvent::SessionUpdate {
                            session_id,
                            update: params,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                // Other notifications are forward-compatible no-ops in this slice.
            }
            ParsedLine::Response { id, result, error } => {
                let method = pending_requests.lock().ok().and_then(|mut m| m.remove(&id));
                let method_name = method.clone().unwrap_or_default();

                if let Some(err) = error {
                    let message = err
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("agent error")
                        .to_string();
                    if tx
                        .send(AcpEvent::RequestFailed {
                            request_id: id,
                            method: method_name,
                            message,
                        })
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }

                let Some(result) = result else {
                    // Response with neither `result` nor `error` — malformed
                    // per JSON-RPC, but don't desync: surface and move on.
                    if tx
                        .send(AcpEvent::RequestFailed {
                            request_id: id,
                            method: method_name,
                            message: "response had neither result nor error".to_string(),
                        })
                        .is_err()
                    {
                        break;
                    }
                    continue;
                };

                let event = match method.as_deref() {
                    Some("initialize") => {
                        was_initialized = true;
                        Some(AcpEvent::Initialized {
                            request_id: id,
                            protocol_version: result
                                .get("protocolVersion")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(PROTOCOL_VERSION),
                            agent_capabilities: result
                                .get("agentCapabilities")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null),
                            agent_info: result
                                .get("agentInfo")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null),
                            auth_methods: result
                                .get("authMethods")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default(),
                        })
                    }
                    Some("session/new") => {
                        result.get("sessionId").and_then(|v| v.as_str()).map(|sid| {
                            AcpEvent::SessionCreated {
                                request_id: id,
                                session_id: sid.to_string(),
                                modes: result.get("modes").cloned(),
                                config_options: result.get("configOptions").cloned(),
                            }
                        })
                    }
                    Some("session/prompt") => Some(AcpEvent::PromptStopped {
                        request_id: id,
                        stop_reason: result
                            .get("stopReason")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                    }),
                    Some("authenticate") => Some(AcpEvent::Authenticated { request_id: id }),
                    // Response to a request we don't track by id anymore
                    // (already timed out, or the id was never ours) —
                    // tolerate and drop, don't desync.
                    _ => None,
                };
                if let Some(event) = event {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
            }
        }
    }

    // Brief pause to let the stderr reader thread finish collecting output,
    // same shape as `lsp.rs`'s `send_exit`.
    std::thread::sleep(std::time::Duration::from_millis(50));
    let stderr_output = stderr_buf.lock().map(|s| s.clone()).unwrap_or_default();
    let _ = tx.send(AcpEvent::AgentExited {
        stderr: stderr_output,
        was_initialized,
    });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- classify_line: pure, no subprocess needed ----

    #[test]
    fn classify_line_request_has_method_and_id() {
        let line = r#"{"jsonrpc":"2.0","id":9001,"method":"fs/read_text_file","params":{"path":"/tmp/x"}}"#;
        match classify_line(line) {
            ParsedLine::AgentToClientRequest { id, method, params } => {
                assert_eq!(id, 9001);
                assert_eq!(method, "fs/read_text_file");
                assert_eq!(params["path"], "/tmp/x");
            }
            other => panic!("expected AgentToClientRequest, got {other:?}"),
        }
    }

    #[test]
    fn classify_line_notification_has_method_no_id() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1"}}"#;
        match classify_line(line) {
            ParsedLine::Notification { method, params } => {
                assert_eq!(method, "session/update");
                assert_eq!(params["sessionId"], "s1");
            }
            other => panic!("expected Notification, got {other:?}"),
        }
    }

    #[test]
    fn classify_line_response_has_id_no_method() {
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"ok":true}}"#;
        match classify_line(line) {
            ParsedLine::Response { id, result, error } => {
                assert_eq!(id, 3);
                assert_eq!(result.unwrap()["ok"], true);
                assert!(error.is_none());
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn classify_line_error_response() {
        let line = r#"{"jsonrpc":"2.0","id":3,"error":{"code":-1,"message":"boom"}}"#;
        match classify_line(line) {
            ParsedLine::Response { id, result, error } => {
                assert_eq!(id, 3);
                assert!(result.is_none());
                assert_eq!(error.unwrap()["message"], "boom");
            }
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn classify_line_tolerates_stringified_id() {
        // Some processes echo ids as JSON strings rather than numbers — the
        // reader must not desync or drop the message.
        let line = r#"{"jsonrpc":"2.0","id":"42","result":{}}"#;
        match classify_line(line) {
            ParsedLine::Response { id, .. } => assert_eq!(id, 42),
            other => panic!("expected Response, got {other:?}"),
        }
    }

    #[test]
    fn classify_line_malformed_json_is_unusable_not_a_panic() {
        assert_eq!(classify_line("not json at all"), ParsedLine::Unusable);
        assert_eq!(classify_line(""), ParsedLine::Unusable);
        assert_eq!(classify_line("   "), ParsedLine::Unusable);
        // Valid JSON, but neither `method` nor `id` — still unusable, not a crash.
        assert_eq!(classify_line(r#"{"jsonrpc":"2.0"}"#), ParsedLine::Unusable);
        assert_eq!(
            classify_line("{ this is not valid json"),
            ParsedLine::Unusable
        );
    }

    // ---- parse_agent_command / session_update_chunk: pure, no subprocess ----

    #[test]
    fn parse_agent_command_splits_plain_whitespace() {
        assert_eq!(
            parse_agent_command("claude-code-acp --flag value"),
            vec!["claude-code-acp", "--flag", "value"]
        );
    }

    #[test]
    fn parse_agent_command_honours_double_quotes() {
        assert_eq!(
            parse_agent_command(r#"/opt/my agent/run.sh "--name=has space" last"#),
            vec!["/opt/my", "agent/run.sh", "--name=has space", "last"]
        );
    }

    #[test]
    fn parse_agent_command_empty_and_whitespace_only_is_empty_argv() {
        assert!(parse_agent_command("").is_empty());
        assert!(parse_agent_command("   \t  ").is_empty());
    }

    // ---- parse_agent_env (#958, ACP-7 agent registry): pure, no subprocess ----

    #[test]
    fn parse_agent_env_splits_key_value_pairs() {
        assert_eq!(
            parse_agent_env(&["FOO=bar".to_string(), "BAZ=qux=extra".to_string()]),
            vec![
                ("FOO".to_string(), "bar".to_string()),
                ("BAZ".to_string(), "qux=extra".to_string()),
            ]
        );
    }

    #[test]
    fn parse_agent_env_skips_entries_with_no_equals() {
        assert_eq!(
            parse_agent_env(&["MALFORMED".to_string(), "OK=1".to_string()]),
            vec![("OK".to_string(), "1".to_string())]
        );
    }

    #[test]
    fn parse_agent_env_empty_is_empty() {
        assert!(parse_agent_env(&[]).is_empty());
    }

    #[test]
    fn session_update_chunk_maps_message_thought_and_user_echo() {
        let msg = serde_json::json!({
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": "hello"},
        });
        assert_eq!(
            session_update_chunk(&msg),
            Some((AcpChunkKind::Message, "hello".to_string()))
        );

        let thought = serde_json::json!({
            "sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": "pondering"},
        });
        assert_eq!(
            session_update_chunk(&thought),
            Some((AcpChunkKind::Thought, "pondering".to_string()))
        );

        let echo = serde_json::json!({
            "sessionUpdate": "user_message_chunk",
            "content": {"type": "text", "text": "hi from another client"},
        });
        assert_eq!(
            session_update_chunk(&echo),
            Some((AcpChunkKind::UserEcho, "hi from another client".to_string()))
        );
    }

    #[test]
    fn session_update_chunk_ignores_tool_call_and_plan_without_panicking() {
        for kind in ["tool_call", "tool_call_update", "plan"] {
            let update = serde_json::json!({"sessionUpdate": kind, "content": {}});
            assert_eq!(session_update_chunk(&update), None, "kind={kind}");
        }
        assert_eq!(session_update_chunk(&serde_json::json!({})), None);
        assert_eq!(
            session_update_chunk(&serde_json::json!({"sessionUpdate": "agent_message_chunk"})),
            Some((AcpChunkKind::Message, String::new())),
            "missing content should yield empty text, not None"
        );
    }

    #[test]
    fn encode_ndjson_line_is_single_line_terminated() {
        let body = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        let encoded = encode_ndjson_line(&body);
        let s = String::from_utf8(encoded).unwrap();
        assert!(s.ends_with('\n'));
        assert_eq!(
            s.matches('\n').count(),
            1,
            "exactly one NDJSON line, no embedded newlines"
        );
        let reparsed: serde_json::Value = serde_json::from_str(s.trim_end()).unwrap();
        assert_eq!(reparsed["method"], "initialize");
    }

    // ---- plan / available_commands_update / current_mode_update / usage_update:
    //      pure, no subprocess (#956, ACP-5) ----

    /// The core acceptance bar: two successive `plan` updates must leave
    /// exactly one plan when applied the way `Engine::acp_handle_session_update`
    /// applies them (`self.acp_plan = entries`, a plain overwrite — never
    /// `.extend`/`.push`). This test proves `parse_plan_update` itself
    /// returns a fresh, independent `Vec` each call (nothing carried over
    /// from a previous parse) so that overwrite-not-append contract is even
    /// possible to uphold at the call site.
    #[test]
    fn parse_plan_update_two_successive_updates_each_fully_replace() {
        let first = serde_json::json!({
            "sessionUpdate": "plan",
            "entries": [{"content": "Write the fix", "status": "in_progress"}],
        });
        let second = serde_json::json!({
            "sessionUpdate": "plan",
            "entries": [
                {"content": "Write the fix", "status": "completed"},
                {"content": "Add tests", "status": "pending"},
            ],
        });

        let mut plan = parse_plan_update(&first).expect("should parse");
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].status, AcpPlanEntryStatus::InProgress);

        // Simulate the engine's actual apply site: a plain overwrite.
        plan = parse_plan_update(&second).expect("should parse");
        assert_eq!(
            plan.len(),
            2,
            "the second update's own entries, not 1+2 accumulated: {plan:?}"
        );
        assert_eq!(plan[0].content, "Write the fix");
        assert_eq!(plan[0].status, AcpPlanEntryStatus::Completed);
        assert_eq!(plan[1].content, "Add tests");
        assert_eq!(plan[1].status, AcpPlanEntryStatus::Pending);
    }

    #[test]
    fn parse_plan_update_rejects_non_plan_and_missing_entries() {
        assert_eq!(
            parse_plan_update(&serde_json::json!({"sessionUpdate": "agent_message_chunk"})),
            None
        );
        assert_eq!(
            parse_plan_update(&serde_json::json!({"sessionUpdate": "plan"})),
            None,
            "no entries array at all is malformed, not a valid empty plan"
        );
    }

    #[test]
    fn parse_plan_update_defaults_missing_status_to_pending_never_completed() {
        let update = serde_json::json!({
            "sessionUpdate": "plan",
            "entries": [{"content": "step one"}, {"content": "step two", "status": "bogus"}],
        });
        let plan = parse_plan_update(&update).expect("should parse");
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].status, AcpPlanEntryStatus::Pending);
        assert_eq!(plan[1].status, AcpPlanEntryStatus::Pending);
    }

    #[test]
    fn plan_to_checklist_text_marks_completed_and_in_progress_distinctly() {
        let entries = vec![
            AcpPlanEntry {
                content: "done step".to_string(),
                status: AcpPlanEntryStatus::Completed,
            },
            AcpPlanEntry {
                content: "active step".to_string(),
                status: AcpPlanEntryStatus::InProgress,
            },
            AcpPlanEntry {
                content: "queued step".to_string(),
                status: AcpPlanEntryStatus::Pending,
            },
        ];
        let text = plan_to_checklist_text(&entries);
        assert!(text.contains("\u{2611} done step"));
        assert!(text.contains("\u{2610} active step (in progress)"));
        assert!(text.contains("\u{2610} queued step"));
        assert!(
            !text.contains("queued step (in progress)"),
            "only the in_progress entry gets the suffix: {text:?}"
        );
    }

    #[test]
    fn parse_available_commands_update_reads_name_and_description() {
        let update = serde_json::json!({
            "sessionUpdate": "available_commands_update",
            "availableCommands": [
                {"name": "commit", "description": "Commit staged changes"},
                {"name": "compact"},
            ],
        });
        let commands = parse_available_commands_update(&update).expect("should parse");
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].name, "commit");
        assert_eq!(commands[0].description, "Commit staged changes");
        assert_eq!(commands[1].name, "compact");
        assert_eq!(
            commands[1].description, "",
            "missing description defaults to empty"
        );
    }

    #[test]
    fn parse_available_commands_update_rejects_wrong_tag() {
        assert_eq!(
            parse_available_commands_update(&serde_json::json!({"sessionUpdate": "plan"})),
            None
        );
    }

    #[test]
    fn parse_session_modes_reads_current_and_available() {
        let modes = serde_json::json!({
            "currentModeId": "code",
            "availableModes": [
                {"id": "code", "name": "Code"},
                {"id": "plan", "name": "Plan"},
            ],
        });
        let (current, list) = parse_session_modes(&modes);
        assert_eq!(current, Some("code".to_string()));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "code");
        assert_eq!(list[0].name, "Code");
    }

    #[test]
    fn parse_session_modes_absent_yields_no_modes_not_an_error() {
        let (current, list) = parse_session_modes(&serde_json::json!(null));
        assert_eq!(current, None);
        assert!(list.is_empty());
    }

    /// #957 (ACP-6): the pure parser side of "the terminal method is
    /// absent when the capability isn't advertised" — an `authMethods`
    /// array with no `type: "terminal"` entry at all (what a real agent
    /// would send when it never saw `auth.terminal: true`) must classify
    /// with zero [`AcpAuthMethodKind::Terminal`] results, never one
    /// conjured from a missing/unrecognized `type`. The wire-level half of
    /// this contract (the client actually sends the capability, and the
    /// fixture actually gates its response on it) lives in
    /// `initialize_auth_methods_includes_terminal_method_when_capability_advertised`
    /// above.
    #[test]
    fn parse_auth_methods_classifies_agent_and_terminal_kinds() {
        let methods = vec![
            serde_json::json!({"id": "api-key", "name": "API Key", "type": "agent"}),
            serde_json::json!({"id": "claude-ai-login", "name": "Claude Subscription", "type": "terminal"}),
        ];
        let parsed = parse_auth_methods(&methods);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].kind, AcpAuthMethodKind::Agent);
        assert_eq!(parsed[1].kind, AcpAuthMethodKind::Terminal);

        // An agent that never offers a terminal method (because it never
        // saw the capability, or simply doesn't support subscription
        // login) yields no Terminal-kind entries at all.
        let agent_only =
            vec![serde_json::json!({"id": "api-key", "name": "API Key", "type": "agent"})];
        let parsed = parse_auth_methods(&agent_only);
        assert!(parsed.iter().all(|m| m.kind == AcpAuthMethodKind::Agent));
    }

    #[test]
    fn parse_auth_methods_defaults_missing_type_to_agent_and_drops_missing_id() {
        let methods = vec![
            serde_json::json!({"id": "mystery", "name": "Mystery"}),
            serde_json::json!({"name": "No id at all", "type": "terminal"}),
        ];
        let parsed = parse_auth_methods(&methods);
        assert_eq!(
            parsed.len(),
            1,
            "entry missing id must be dropped: {parsed:?}"
        );
        assert_eq!(parsed[0].id, "mystery");
        assert_eq!(
            parsed[0].kind,
            AcpAuthMethodKind::Agent,
            "unrecognized/missing type must default to Agent, never Terminal"
        );
    }

    #[test]
    fn parse_auth_methods_name_falls_back_to_id() {
        let methods = vec![serde_json::json!({"id": "api-key", "type": "agent"})];
        let parsed = parse_auth_methods(&methods);
        assert_eq!(parsed[0].name, "api-key");
    }

    #[test]
    fn parse_current_mode_update_reads_mode_id() {
        let update = serde_json::json!({
            "sessionUpdate": "current_mode_update",
            "currentModeId": "plan",
        });
        assert_eq!(parse_current_mode_update(&update), Some("plan".to_string()));
        assert_eq!(
            parse_current_mode_update(&serde_json::json!({"sessionUpdate": "plan"})),
            None
        );
    }

    #[test]
    fn parse_usage_update_reads_nested_and_flat_shapes() {
        let nested = serde_json::json!({
            "sessionUpdate": "usage_update",
            "usage": {"inputTokens": 120, "outputTokens": 45, "totalCostUsd": 0.0067},
        });
        let usage = parse_usage_update(&nested).expect("should parse");
        assert_eq!(usage.input_tokens, Some(120));
        assert_eq!(usage.output_tokens, Some(45));
        assert_eq!(usage.total_cost_usd, Some(0.0067));

        // Tolerate an alternate flattened/renamed shape too.
        let flat = serde_json::json!({
            "sessionUpdate": "usage_update",
            "promptTokens": 10,
            "cost": 0.001,
        });
        let usage2 = parse_usage_update(&flat).expect("should parse");
        assert_eq!(usage2.input_tokens, Some(10));
        assert_eq!(usage2.output_tokens, None);
        assert_eq!(usage2.total_cost_usd, Some(0.001));
    }

    #[test]
    fn parse_usage_update_rejects_wrong_tag_and_empty_usage() {
        assert_eq!(
            parse_usage_update(&serde_json::json!({"sessionUpdate": "plan"})),
            None
        );
        assert_eq!(
            parse_usage_update(&serde_json::json!({"sessionUpdate": "usage_update", "usage": {}})),
            None,
            "no recognizable field at all should not fabricate a report"
        );
    }

    #[test]
    fn format_usage_summary_only_includes_fields_present() {
        let both = AcpUsage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            total_cost_usd: Some(0.02),
        };
        assert_eq!(format_usage_summary(&both), "10+5 tok \u{b7} $0.0200");

        let cost_only = AcpUsage {
            input_tokens: None,
            output_tokens: None,
            total_cost_usd: Some(1.5),
        };
        assert_eq!(format_usage_summary(&cost_only), "$1.5000");

        let nothing = AcpUsage {
            input_tokens: None,
            output_tokens: None,
            total_cost_usd: None,
        };
        assert_eq!(format_usage_summary(&nothing), "");
    }

    // ---- parse_request_permission / permission_outcome_*: pure, no subprocess (#953, ACP-2) ----

    #[test]
    fn parse_request_permission_reads_tool_call_and_options() {
        let params = serde_json::json!({
            "sessionId": "sess-1",
            "toolCall": {
                "title": "Edit src/main.rs",
                "kind": "edit",
                "locations": [{"path": "src/main.rs", "line": 42}, {"path": "src/lib.rs"}],
            },
            "options": [
                {"optionId": "allow-once", "name": "Allow Once", "kind": "allow_once"},
                {"optionId": "allow-always", "name": "Always Allow", "kind": "allow_always"},
                {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"},
            ],
        });
        let req = parse_request_permission(&params).expect("should parse");
        assert_eq!(req.session_id, "sess-1");
        assert_eq!(req.tool_call.title, "Edit src/main.rs");
        assert_eq!(req.tool_call.kind, "edit");
        assert_eq!(
            req.tool_call.locations,
            vec![
                ("src/main.rs".to_string(), Some(42)),
                ("src/lib.rs".to_string(), None),
            ]
        );
        assert_eq!(req.options.len(), 3);
        assert_eq!(req.options[0].option_id, "allow-once");
        assert_eq!(req.options[0].name, "Allow Once");
        assert_eq!(req.options[0].kind, "allow_once");
        assert_eq!(req.options[1].kind, "allow_always");
        assert_eq!(req.options[2].kind, "reject_once");
    }

    #[test]
    fn parse_request_permission_defaults_missing_title_kind_and_locations() {
        let params = serde_json::json!({
            "sessionId": "sess-1",
            "toolCall": {},
            "options": [{"optionId": "x", "name": "Go", "kind": "allow_once"}],
        });
        let req = parse_request_permission(&params).expect("should parse");
        assert_eq!(req.tool_call.title, "Tool call");
        assert_eq!(req.tool_call.kind, "other");
        assert!(req.tool_call.locations.is_empty());
    }

    /// #953 review (non-blocking concern): a missing per-option `kind` must
    /// default to something neutral, never to `"allow_once"` — the most
    /// permissive category — since that would let a request with a missing
    /// `kind` silently match `Engine::acp_remembered_decisions`'s
    /// `allow_`-prefix lookup as if a human had already approved it.
    #[test]
    fn parse_request_permission_defaults_missing_option_kind_to_neutral_not_allow_once() {
        let params = serde_json::json!({
            "sessionId": "sess-1",
            "toolCall": {"title": "t", "kind": "edit"},
            "options": [{"optionId": "x", "name": "Go"}],
        });
        let req = parse_request_permission(&params).expect("should parse");
        assert_eq!(req.options.len(), 1);
        assert_ne!(
            req.options[0].kind, "allow_once",
            "a missing option kind must not default to the most permissive \
             category"
        );
        assert!(
            !req.options[0].kind.starts_with("allow_")
                && !req.options[0].kind.starts_with("reject_"),
            "a missing option kind must not match either remembered-decision \
             prefix: {:?}",
            req.options[0].kind
        );
    }

    #[test]
    fn parse_request_permission_rejects_malformed_shapes() {
        // No sessionId.
        assert!(parse_request_permission(&serde_json::json!({
            "toolCall": {}, "options": [{"optionId": "x", "name": "n", "kind": "allow_once"}],
        }))
        .is_none());
        // No toolCall.
        assert!(parse_request_permission(&serde_json::json!({
            "sessionId": "s", "options": [{"optionId": "x", "name": "n", "kind": "allow_once"}],
        }))
        .is_none());
        // No options array.
        assert!(parse_request_permission(&serde_json::json!({
            "sessionId": "s", "toolCall": {},
        }))
        .is_none());
        // Empty options array — nothing a human could select.
        assert!(parse_request_permission(&serde_json::json!({
            "sessionId": "s", "toolCall": {}, "options": [],
        }))
        .is_none());
        // An option missing optionId must drop the whole request (never a
        // dialog with an unselectable/unanswerable button).
        assert!(parse_request_permission(&serde_json::json!({
            "sessionId": "s", "toolCall": {}, "options": [{"name": "n", "kind": "allow_once"}],
        }))
        .is_none());
    }

    #[test]
    fn permission_outcome_helpers_match_the_acp_v1_wire_shape() {
        assert_eq!(
            permission_outcome_selected("opt-1"),
            serde_json::json!({"outcome": {"outcome": "selected", "optionId": "opt-1"}})
        );
        assert_eq!(
            permission_outcome_cancelled(),
            serde_json::json!({"outcome": {"outcome": "cancelled"}})
        );
    }

    // ---- tool_call / tool_call_update: pure, no subprocess (#955, ACP-4) ----

    #[test]
    fn parse_tool_call_reads_title_kind_status_locations_and_content() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "tc-1",
            "title": "Edit src/main.rs",
            "kind": "edit",
            "status": "in_progress",
            "locations": [{"path": "src/main.rs", "line": 10}],
            "content": [
                {"type": "content", "content": {"type": "text", "text": "hello"}},
                {"type": "diff", "path": "src/main.rs", "oldText": "old\n", "newText": "new\n"},
                {"type": "terminal", "terminalId": "t1"},
            ],
        });
        let call = parse_tool_call(&update).expect("should parse");
        assert_eq!(call.id, "tc-1");
        assert_eq!(call.title, "Edit src/main.rs");
        assert_eq!(call.kind, "edit");
        assert_eq!(call.status, AcpToolCallStatus::InProgress);
        assert_eq!(call.locations, vec![("src/main.rs".to_string(), Some(10))]);
        assert_eq!(call.content.len(), 3);
        assert_eq!(
            call.content[0],
            AcpToolCallContentBlock::Text("hello".to_string())
        );
        assert_eq!(
            call.content[1],
            AcpToolCallContentBlock::Diff {
                path: "src/main.rs".to_string(),
                old_text: Some("old\n".to_string()),
                new_text: "new\n".to_string(),
            }
        );
        assert_eq!(call.content[2], AcpToolCallContentBlock::Terminal);
    }

    #[test]
    fn parse_tool_call_defaults_missing_kind_and_status() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "tc-1",
            "title": "Do a thing",
        });
        let call = parse_tool_call(&update).expect("should parse");
        assert_eq!(call.kind, "other");
        assert_eq!(call.status, AcpToolCallStatus::Pending);
        assert!(call.locations.is_empty());
        assert!(call.content.is_empty());
    }

    #[test]
    fn parse_tool_call_requires_id_and_title() {
        assert!(parse_tool_call(&serde_json::json!({
            "sessionUpdate": "tool_call", "title": "t",
        }))
        .is_none());
        assert!(parse_tool_call(&serde_json::json!({
            "sessionUpdate": "tool_call", "toolCallId": "tc-1",
        }))
        .is_none());
        // Wrong kind of update entirely.
        assert!(parse_tool_call(&serde_json::json!({
            "sessionUpdate": "plan", "toolCallId": "tc-1", "title": "t",
        }))
        .is_none());
    }

    /// A `diff` block with `oldText: null` (a new file, per the ACP v1
    /// schema's `string | null`) must parse to `old_text: None`, not a
    /// dropped block or an `Ok(Some(String::new()))` that erases the
    /// distinction.
    #[test]
    fn parse_tool_call_diff_block_null_old_text_is_new_file() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "tc-1",
            "title": "Create file",
            "content": [
                {"type": "diff", "path": "src/new.rs", "oldText": null, "newText": "fn main() {}\n"},
            ],
        });
        let call = parse_tool_call(&update).expect("should parse");
        assert_eq!(
            call.content[0],
            AcpToolCallContentBlock::Diff {
                path: "src/new.rs".to_string(),
                old_text: None,
                new_text: "fn main() {}\n".to_string(),
            }
        );
    }

    #[test]
    fn parse_tool_call_update_reads_status_and_appends_content() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "tc-1",
            "status": "completed",
            "content": [
                {"type": "content", "content": {"type": "text", "text": "done"}},
            ],
        });
        let upd = parse_tool_call_update(&update).expect("should parse");
        assert_eq!(upd.id, "tc-1");
        assert_eq!(upd.status, Some(AcpToolCallStatus::Completed));
        assert_eq!(
            upd.content,
            Some(vec![AcpToolCallContentBlock::Text("done".to_string())])
        );
    }

    #[test]
    fn parse_tool_call_update_status_only_leaves_content_none() {
        let update = serde_json::json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "tc-1",
            "status": "failed",
        });
        let upd = parse_tool_call_update(&update).expect("should parse");
        assert_eq!(upd.status, Some(AcpToolCallStatus::Failed));
        assert_eq!(upd.content, None);
    }

    #[test]
    fn parse_tool_call_update_requires_id() {
        assert!(parse_tool_call_update(&serde_json::json!({
            "sessionUpdate": "tool_call_update", "status": "completed",
        }))
        .is_none());
    }

    #[test]
    fn tool_call_summary_line_includes_glyph_kind_title_and_locations() {
        let call = AcpToolCall {
            id: "tc-1".to_string(),
            title: "Edit src/main.rs".to_string(),
            kind: "edit".to_string(),
            status: AcpToolCallStatus::InProgress,
            locations: vec![("src/main.rs".to_string(), Some(10))],
            content: vec![],
        };
        let line = tool_call_summary_line(&call);
        assert_eq!(
            line,
            "[~] edit: Edit src/main.rs\n    \u{2192} src/main.rs:10"
        );
    }

    #[test]
    fn tool_call_status_glyph_transitions_are_visually_distinct() {
        let glyphs: Vec<&str> = [
            AcpToolCallStatus::Pending,
            AcpToolCallStatus::InProgress,
            AcpToolCallStatus::Completed,
            AcpToolCallStatus::Failed,
        ]
        .iter()
        .map(|s| s.glyph())
        .collect();
        let unique: std::collections::HashSet<&str> = glyphs.iter().copied().collect();
        assert_eq!(
            unique.len(),
            glyphs.len(),
            "every status must have a distinct glyph"
        );
    }

    // ---- fs/read_text_file, fs/write_text_file: pure, no subprocess (#954, ACP-3) ----

    #[test]
    fn parse_read_text_file_params_reads_path_line_and_limit() {
        let params = serde_json::json!({
            "sessionId": "sess-1",
            "path": "/tmp/x.txt",
            "line": 3,
            "limit": 10,
        });
        let req = parse_read_text_file_params(&params).expect("should parse");
        assert_eq!(req.session_id, "sess-1");
        assert_eq!(req.path, "/tmp/x.txt");
        assert_eq!(req.line, Some(3));
        assert_eq!(req.limit, Some(10));
    }

    #[test]
    fn parse_read_text_file_params_line_and_limit_are_optional() {
        let params = serde_json::json!({"sessionId": "s", "path": "/tmp/x.txt"});
        let req = parse_read_text_file_params(&params).expect("should parse");
        assert_eq!(req.line, None);
        assert_eq!(req.limit, None);
    }

    #[test]
    fn parse_read_text_file_params_rejects_missing_path_or_session() {
        assert!(parse_read_text_file_params(&serde_json::json!({"path": "/tmp/x"})).is_none());
        assert!(parse_read_text_file_params(&serde_json::json!({"sessionId": "s"})).is_none());
    }

    #[test]
    fn parse_write_text_file_params_reads_path_and_content() {
        let params = serde_json::json!({
            "sessionId": "sess-1",
            "path": "/tmp/x.txt",
            "content": "hello\nworld\n",
        });
        let req = parse_write_text_file_params(&params).expect("should parse");
        assert_eq!(req.session_id, "sess-1");
        assert_eq!(req.path, "/tmp/x.txt");
        assert_eq!(req.content, "hello\nworld\n");
    }

    #[test]
    fn parse_write_text_file_params_rejects_missing_fields() {
        assert!(parse_write_text_file_params(&serde_json::json!({
            "sessionId": "s", "path": "/tmp/x",
        }))
        .is_none());
        assert!(parse_write_text_file_params(&serde_json::json!({
            "sessionId": "s", "content": "c",
        }))
        .is_none());
    }

    #[test]
    fn read_text_file_result_matches_the_acp_v1_wire_shape() {
        assert_eq!(
            read_text_file_result("hello"),
            serde_json::json!({"content": "hello"})
        );
    }

    #[test]
    fn select_text_lines_returns_everything_when_line_and_limit_are_absent() {
        let content = "a\nb\nc\n";
        assert_eq!(select_text_lines(content, None, None), content);
    }

    #[test]
    fn select_text_lines_honours_line_and_limit() {
        let content = "one\ntwo\nthree\nfour\nfive";
        // 1-based `line: 2` starts at "two".
        assert_eq!(
            select_text_lines(content, Some(2), None),
            "two\nthree\nfour\nfive"
        );
        assert_eq!(select_text_lines(content, Some(2), Some(2)), "two\nthree");
        assert_eq!(select_text_lines(content, None, Some(1)), "one");
    }

    #[test]
    fn select_text_lines_clamps_out_of_range_line_and_limit_without_panicking() {
        let content = "one\ntwo";
        assert_eq!(select_text_lines(content, Some(100), None), "");
        assert_eq!(select_text_lines(content, Some(1), Some(100)), "one\ntwo");
        assert_eq!(select_text_lines(content, Some(0), None), "one\ntwo");
    }

    #[test]
    fn resolve_path_within_roots_accepts_a_path_inside_an_existing_root() {
        let dir = std::env::temp_dir().join(format!("acp3-root-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("existing.txt");
        std::fs::write(&file, "hi").unwrap();

        let resolved = resolve_path_within_roots(&file, &[dir.clone()])
            .expect("path inside the root should resolve");
        assert_eq!(resolved, file.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_path_within_roots_accepts_a_not_yet_existing_file_inside_the_root() {
        let dir = std::env::temp_dir().join(format!("acp3-newfile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("brand-new.txt"); // does not exist yet

        let resolved = resolve_path_within_roots(&file, &[dir.clone()])
            .expect("a not-yet-existing file inside the root should still resolve");
        assert_eq!(resolved, dir.canonicalize().unwrap().join("brand-new.txt"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_path_within_roots_rejects_a_path_outside_every_root() {
        let dir = std::env::temp_dir().join(format!("acp3-outside-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let outside = std::env::temp_dir().join(format!("acp3-elsewhere-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        let target = outside.join("secret.txt");
        std::fs::write(&target, "nope").unwrap();

        let err = resolve_path_within_roots(&target, &[dir.clone()])
            .expect_err("a path outside the only root must be refused");
        assert!(
            err.contains("outside the session workspace"),
            "error should name the reason: {err:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    /// The traversal case the whole function exists for: a lexical `..`
    /// escape must not slip past a naive `starts_with(root)` check just
    /// because the un-normalized string happens to start with the root.
    #[test]
    fn resolve_path_within_roots_rejects_dot_dot_traversal_out_of_the_root() {
        let dir = std::env::temp_dir().join(format!("acp3-traversal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let escaping = dir.join("..").join(
            std::env::temp_dir()
                .file_name()
                .map(|_| "escaped.txt")
                .unwrap_or("escaped.txt"),
        );

        let err = resolve_path_within_roots(&escaping, &[dir.clone()]);
        assert!(
            err.is_err(),
            "`..` must not be able to walk out of the only allowed root: {err:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- Integration: fake NDJSON echo agent subprocess ----
    //
    // `tests/fixtures/fake_acp_agent.sh` is the fixture the whole ACP track
    // depends on (per #951's "fixture this slice owes the rest of the
    // track") — deterministic canned responses keyed by method name via
    // plain `/bin/sh` + `case`, no jq/python/node, so it runs in CI (#951:
    // "CI has no Node and no agent login").

    #[cfg(unix)]
    fn fixture_path() -> std::path::PathBuf {
        std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/fake_acp_agent.sh"
        ))
    }

    #[cfg(unix)]
    fn spawn_fixture(extra_env: &[(&str, &str)]) -> AcpClient {
        // Deliberately NOT `core::terminal::shell_command()`: that seam picks
        // an interpreter to run a *user-supplied command string* via `-c`
        // ("the user's shell"). Here we're invoking a fixed, checked-in
        // script (`fake_acp_agent.sh`) that is itself `#!/bin/sh` and
        // documents its use of POSIX-only features (`case...esac`, `sed`,
        // shell functions) — it is not portable to `$SHELL` being e.g. fish
        // or csh/tcsh. "sh" must stay hardcoded here (see #1255).
        let argv = vec![
            "sh".to_string(),
            fixture_path().to_string_lossy().into_owned(),
        ];
        let cwd = std::env::temp_dir();
        AcpClient::spawn_with_env(&argv, &cwd, extra_env).expect("fixture agent should spawn")
    }

    /// Poll until at least one event has arrived or the deadline passes.
    /// The fake agent replies synchronously off a blocking `read`, so the
    /// only thing being ridden out here is process/pipe scheduling latency —
    /// but see [`TEST_DEADLINE`] for why "scheduling latency" is not
    /// automatically small.
    #[cfg(unix)]
    fn poll_until(client: &mut AcpClient, deadline: std::time::Duration) -> Vec<AcpEvent> {
        let start = std::time::Instant::now();
        loop {
            let events = client.poll();
            if !events.is_empty() || start.elapsed() > deadline {
                return events;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// Poll, *accumulating* across drains, until `done` is satisfied by the
    /// events collected so far or the deadline passes.
    ///
    /// Use this instead of a second `poll_until` whenever the events under
    /// test are produced by the agent back-to-back. `AcpClient::poll` drains
    /// whatever the reader thread has queued at that instant, so how many
    /// events land in one call is pure scheduling luck: with
    /// `ACP_FAKE_DIE_AFTER_INIT` the fixture writes its `initialize` response
    /// and exits immediately, so the `Initialized` event and the `AgentExited`
    /// that its EOF produces may arrive in *one* drain or in two. Asserting
    /// "the first poll returns Initialized, the second returns AgentExited"
    /// therefore passes or fails at random — it was the flake seen at #984's
    /// test stage. Nothing about the client's contract promises a one-event-
    /// per-poll cadence, and no deadline can repair that — a drain that
    /// already returned both events leaves the *next* one empty however long
    /// you wait. Accumulating is the fix; the property under test is that
    /// both events *arrive*, in order, not how they are batched.
    #[cfg(unix)]
    fn poll_collecting_until(
        client: &mut AcpClient,
        deadline: std::time::Duration,
        done: impl Fn(&[AcpEvent]) -> bool,
    ) -> Vec<AcpEvent> {
        let start = std::time::Instant::now();
        let mut all = Vec::new();
        loop {
            all.extend(client.poll());
            if done(&all) || start.elapsed() > deadline {
                return all;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// How long a fixture-backed test waits for an event before giving up.
    ///
    /// Deliberately generous rather than "roughly how long the fixture takes
    /// on an idle box". Every wait loop in this module exits the instant its
    /// condition holds, so the bound is only ever reached on the *failing*
    /// path — which means a larger value costs a passing run exactly nothing
    /// and only buys headroom on a loaded one. That headroom is the point:
    /// a full `cargo test` drives the GTK harness, ~2.7k lib tests and the
    /// nvim conformance oracles concurrently, and a `sh` fork+exec plus a
    /// pipe round-trip can be descheduled for orders of magnitude longer
    /// there than the couple of milliseconds it costs standalone. The two
    /// death-path tests (here and in `engine::acp_ops`) were the ones that
    /// went red at #984's test stage, so they are the ones that must not be
    /// sitting near their bound.
    #[cfg(unix)]
    const TEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

    #[cfg(unix)]
    #[test]
    fn full_session_lifecycle_reaches_end_turn() {
        // Acceptance: initialize -> session/new -> session/prompt ->
        // stopReason: end_turn against the fake agent, AND a scripted
        // agent -> client request dispatched mid-turn is parked and
        // answered out of band, with the reply reaching the agent (the
        // fixture blocks on it before completing the turn — if the reply
        // never lands, this test times out instead of passing).
        let mut client = spawn_fixture(&[]);

        client.initialize();
        let events = poll_until(&mut client, TEST_DEADLINE);
        assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
        match &events[0] {
            AcpEvent::Initialized {
                protocol_version,
                agent_info,
                ..
            } => {
                assert_eq!(*protocol_version, PROTOCOL_VERSION);
                assert_eq!(agent_info["name"], "fake-acp-agent");
            }
            other => panic!("expected Initialized, got {other:?}"),
        }

        client.new_session(&std::env::temp_dir(), vec![]);
        let events = poll_until(&mut client, TEST_DEADLINE);
        let session_id = match &events[0] {
            AcpEvent::SessionCreated { session_id, .. } => session_id.clone(),
            other => panic!("expected SessionCreated, got {other:?}"),
        };
        assert_eq!(session_id, "sess-1");

        client.prompt(
            &session_id,
            vec![serde_json::json!({"type": "text", "text": "hi"})],
        );

        // First: a session/update notification, then the parked client
        // request (fs/read_text_file). Poll until we've seen the request.
        let mut saw_update = false;
        let mut client_request_id = None;
        let start = std::time::Instant::now();
        while client_request_id.is_none() && start.elapsed() < TEST_DEADLINE {
            for event in client.poll() {
                match event {
                    AcpEvent::SessionUpdate { .. } => saw_update = true,
                    AcpEvent::ClientRequest {
                        request_id, method, ..
                    } => {
                        assert_eq!(method, "fs/read_text_file", "dispatched by method name");
                        client_request_id = Some(request_id);
                    }
                    other => panic!("unexpected event while awaiting client request: {other:?}"),
                }
            }
            if client_request_id.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        assert!(
            saw_update,
            "expected a session/update notification before the client request"
        );
        let request_id = client_request_id.expect("fixture should have issued a client request");
        assert_eq!(client.pending_client_request_count(), 1);

        // Answer it later, out of band — not inline with the dispatch above.
        client.respond_to_client_request(
            request_id,
            Ok(serde_json::json!({"content": "fake file contents"})),
        );
        assert_eq!(client.pending_client_request_count(), 0);

        // The fixture was blocked on that reply; now it should finish the turn.
        let events = poll_until(&mut client, TEST_DEADLINE);
        assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
        match &events[0] {
            AcpEvent::PromptStopped { stop_reason, .. } => assert_eq!(stop_reason, "end_turn"),
            other => panic!("expected PromptStopped, got {other:?}"),
        }
    }

    /// #954 (ACP-3): the `fs.readTextFile`/`fs.writeTextFile`
    /// clientCapabilities must actually reach the wire in the `initialize`
    /// request, not just exist as a `serde_json::json!` literal nobody
    /// sends. The fixture echoes back whether it saw each flag — see its
    /// own doc comment — so this is a real round trip through
    /// `AcpClient::initialize`, not a unit test of JSON construction.
    #[cfg(unix)]
    #[test]
    fn initialize_advertises_fs_capabilities_the_agent_actually_receives() {
        let mut client = spawn_fixture(&[]);
        client.initialize();
        let events = poll_until(&mut client, TEST_DEADLINE);
        assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
        match &events[0] {
            AcpEvent::Initialized { agent_info, .. } => {
                assert_eq!(
                    agent_info["sawReadCap"], true,
                    "the agent's initialize handler never saw readTextFile:true \
                     on the wire: {agent_info:?}"
                );
                assert_eq!(
                    agent_info["sawWriteCap"], true,
                    "the agent's initialize handler never saw writeTextFile:true \
                     on the wire: {agent_info:?}"
                );
            }
            other => panic!("expected Initialized, got {other:?}"),
        }
    }

    /// #957 (ACP-6): `auth.terminal: true` must actually reach the wire in
    /// the `initialize` request — same "the capability flag isn't just a
    /// `serde_json::json!` literal nobody sends" contract as the fs/* test
    /// above, and the same fixture echo mechanism
    /// (`agentInfo.sawAuthTerminalCap`).
    #[cfg(unix)]
    #[test]
    fn initialize_advertises_auth_terminal_capability_the_agent_actually_receives() {
        let mut client = spawn_fixture(&[]);
        client.initialize();
        let events = poll_until(&mut client, TEST_DEADLINE);
        assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
        match &events[0] {
            AcpEvent::Initialized { agent_info, .. } => {
                assert_eq!(
                    agent_info["sawAuthTerminalCap"], true,
                    "the agent's initialize handler never saw auth.terminal:true \
                     on the wire: {agent_info:?}"
                );
            }
            other => panic!("expected Initialized, got {other:?}"),
        }
    }

    /// #957 (ACP-6): with `$ACP_FAKE_AUTH_METHODS` set, the fixture's
    /// `authMethods` includes a `type: "terminal"` entry *because* it saw
    /// `auth.terminal: true` on the wire (this client always sends it —
    /// see the test above) — a real, request-driven gate, not a hardcoded
    /// stub. `parse_auth_methods` must in turn classify that entry as
    /// [`AcpAuthMethodKind::Terminal`] and the "api-key" entry as
    /// [`AcpAuthMethodKind::Agent`].
    #[cfg(unix)]
    #[test]
    fn initialize_auth_methods_includes_terminal_method_when_capability_advertised() {
        let mut client = spawn_fixture(&[("ACP_FAKE_AUTH_METHODS", "1")]);
        client.initialize();
        let events = poll_until(&mut client, TEST_DEADLINE);
        assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
        match &events[0] {
            AcpEvent::Initialized { auth_methods, .. } => {
                let methods = parse_auth_methods(auth_methods);
                assert_eq!(methods.len(), 2, "expected two auth methods: {methods:?}");
                assert_eq!(methods[0].id, "api-key");
                assert_eq!(methods[0].kind, AcpAuthMethodKind::Agent);
                assert_eq!(methods[1].id, "claude-ai-login");
                assert_eq!(methods[1].name, "Claude Subscription");
                assert_eq!(methods[1].kind, AcpAuthMethodKind::Terminal);
            }
            other => panic!("expected Initialized, got {other:?}"),
        }
    }

    /// #957 (ACP-6): a `type: "agent"` auth method answers via the plain
    /// `authenticate {methodId}` RPC — a real round trip through
    /// [`AcpClient::authenticate`], not just JSON construction.
    #[cfg(unix)]
    #[test]
    fn authenticate_round_trip_succeeds() {
        let mut client = spawn_fixture(&[]);
        client.authenticate("api-key");
        let events = poll_until(&mut client, TEST_DEADLINE);
        assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
        assert!(
            matches!(events[0], AcpEvent::Authenticated { .. }),
            "expected Authenticated, got {:?}",
            events[0]
        );
    }

    /// A failed `authenticate` call must surface as an ordinary
    /// [`AcpEvent::RequestFailed`] (method `"authenticate"`) — never a
    /// panic or a silently swallowed response — so
    /// `Engine::poll_acp`'s existing generic error handling clears the
    /// panel's busy state exactly as it does for any other failed
    /// handshake request.
    #[cfg(unix)]
    #[test]
    fn authenticate_failure_surfaces_as_request_failed() {
        let mut client = spawn_fixture(&[("ACP_FAKE_AUTH_FAIL", "1")]);
        client.authenticate("api-key");
        let events = poll_until(&mut client, TEST_DEADLINE);
        assert_eq!(events.len(), 1, "expected exactly one event: {events:?}");
        match &events[0] {
            AcpEvent::RequestFailed { method, .. } => assert_eq!(method, "authenticate"),
            other => panic!("expected RequestFailed, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn agent_death_mid_session_surfaces_as_event_no_panic() {
        let mut client = spawn_fixture(&[("ACP_FAKE_DIE_AFTER_INIT", "1")]);
        client.initialize();

        // The fixture answers `initialize` and exits in the same breath, so
        // `Initialized` and the `AgentExited` its EOF produces may be drained
        // together or separately — see `poll_collecting_until`'s doc. Collect
        // until the death shows up, then assert on the ordered sequence.
        let events = poll_collecting_until(&mut client, TEST_DEADLINE, |seen| {
            seen.iter()
                .any(|e| matches!(e, AcpEvent::AgentExited { .. }))
        });

        // First event: Initialized (the fixture replies before dying).
        assert!(
            matches!(events.first(), Some(AcpEvent::Initialized { .. })),
            "expected Initialized first within {TEST_DEADLINE:?}, got {events:?}"
        );

        // Then: the process death must surface as AgentExited, not a hang
        // or a panic in this test thread.
        match events
            .iter()
            .find(|e| matches!(e, AcpEvent::AgentExited { .. }))
        {
            Some(AcpEvent::AgentExited {
                was_initialized, ..
            }) => assert!(*was_initialized),
            _ => panic!("expected an AgentExited event within {TEST_DEADLINE:?}, got {events:?}"),
        }

        // No orphan process left behind. `AgentExited` is only emitted once
        // the agent's stdout hits EOF, which the kernel does not deliver
        // until the process is gone, so this is close to a formality — but
        // `try_wait` can still report `None` for the sliver between the
        // child calling `exit` and becoming reapable. Poll for it instead of
        // sleeping a fixed 100ms and hoping that sliver fitted inside.
        let start = std::time::Instant::now();
        while !client.has_exited() && start.elapsed() < TEST_DEADLINE {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            client.has_exited(),
            "agent process should have exited on its own within {TEST_DEADLINE:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn malformed_line_and_stderr_noise_do_not_desync_the_reader() {
        // The fixture always logs a line to stderr on startup, and with
        // this env var set it also emits one non-JSON line on *stdout*
        // right before its real `initialize` response. Neither should
        // prevent the real response from parsing correctly.
        let mut client = spawn_fixture(&[("ACP_FAKE_EMIT_GARBAGE", "1")]);
        client.initialize();
        let events = poll_until(&mut client, TEST_DEADLINE);
        assert_eq!(
            events.len(),
            1,
            "garbage line should be skipped, not surfaced: {events:?}"
        );
        assert!(matches!(events[0], AcpEvent::Initialized { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn unrecognized_notification_method_is_ignored_not_fatal() {
        // classify_line already covers the pure-parsing side of this; here
        // we confirm the reader thread's handling of a method it doesn't
        // recognize (anything other than session/update) doesn't wedge the
        // stream — the next real message still comes through.
        let mut client = spawn_fixture(&[]);
        client.initialize();
        let events = poll_until(&mut client, TEST_DEADLINE);
        assert!(matches!(events.first(), Some(AcpEvent::Initialized { .. })));
        // session/cancel is a fire-and-forget notification the fixture
        // acknowledges with nothing; confirm the client survives sending
        // one with no matching response ever arriving.
        client.cancel("no-such-session");
        // If the reader thread wedged, this poll would eventually return
        // something bogus or the test would hang; a clean empty drain
        // within the deadline is the expected steady state.
        let events = poll_until(&mut client, std::time::Duration::from_millis(300));
        assert!(
            events.is_empty(),
            "no event expected for a plain notification round-trip: {events:?}"
        );
    }
}
