//! `AcpSession`: one live (or freshly-opened, not-yet-spawned) ACP
//! conversation — process handle, protocol bookkeeping, and transcript —
//! so `Engine` can hold **several** of them at once (#1463).
//!
//! Before #1463, all of the fields below lived directly on `Engine` as
//! flat `acp_*`/`ai_messages`/`ai_streaming` fields, one of each — a
//! single foreground conversation, full stop. This module bundles that
//! exact same set of fields into one struct so `Engine::acp_sessions:
//! Vec<AcpSession>` can hold N of them, each independently spawned,
//! streaming, and reviewable.
//!
//! ## What stayed on `Engine`, and why
//!
//! Not everything `acp_`-prefixed moved here. Several fields were already
//! documented as **not** session-scoped before this change — moving them
//! would be a behavior change of its own, not a mechanical carry-over:
//! - `acp_session_index` (the cross-session `:AiSessions` resume picker)
//!   and `acp_pending_resume`/`acp_startup_reopen_attempted` — bookkeeping
//!   *about* sessions in general, not state belonging to any one of them.
//! - `acp_pending_attachment`/`acp_manual_attachments` — composed content
//!   for whichever prompt is about to be sent, already documented as "not
//!   session-scoped" pre-#1463.
//! - `change_review` — shared with the non-ACP change-review surface
//!   (`review_ops.rs`); at most one review dialog is open at a time
//!   regardless of which session (or non-ACP source) proposed it.
//! - `ai_has_focus`/`ai_chat`/`ai_rx`/`ai_ghost_*`/`ai_completion_*` — AI
//!   panel widget/legacy-completion state, not ACP protocol or transcript
//!   state.
//!
//! ## Polling every session, not just the active one
//!
//! The whole point of #1463 is that a backgrounded session keeps working
//! while another is in the foreground — an agent mid-turn must keep
//! having its stdout drained even while the user is looking at a
//! different tab, or its turn stalls (and, for a real subprocess, its
//! stdout pipe eventually fills and blocks the agent outright). See
//! `Engine::poll_acp`, which now loops over every entry in `acp_sessions`
//! — not just `acp_active_session` — reusing the exact per-event handling
//! this module's fields used to receive only in the single-session shape.

use std::collections::HashMap;

use super::acp::{
    AcpAuthMethod, AcpAvailableCommand, AcpChunkKind, AcpClient, AcpMcpCapabilities,
    AcpPermissionRequest, AcpPlanEntry, AcpPromptCapabilities, AcpSessionMode, AcpToolCall,
    AcpUsage,
};
use super::ai::AiMessage;

/// One ACP conversation: its own agent process (or none yet), its own
/// `session/new` id, its own transcript, its own in-flight protocol state.
///
/// A freshly-created slot (`AcpSession::new`) has no client and an empty
/// transcript — exactly the state `Engine` used to start in before any
/// message was ever sent. `:AiNew` produces one of these; the first
/// `ai_send_message` against it spawns the agent, same as before #1463.
#[derive(Default)]
pub struct AcpSession {
    /// Human-readable label for this session's tab (the agent name active
    /// when the session was opened — see `Engine::acp_new_session`).
    /// Empty until the first message is sent (mirrors "no agent chosen
    /// yet"); `Engine::acp_session_tab_label` falls back to "New session"
    /// for an empty label.
    pub label: String,

    pub client: Option<AcpClient>,
    pub session_id: Option<String>,
    pub pending_prompt: Option<String>,
    pub pending_prompt_display: Option<String>,
    pub streaming_turn: Option<(usize, AcpChunkKind)>,
    pub pending_permission: Option<(i64, AcpPermissionRequest)>,
    pub remembered_decisions: HashMap<String, bool>,
    pub plan: Vec<AcpPlanEntry>,
    pub available_commands: Vec<AcpAvailableCommand>,
    pub command_completion_idx: usize,
    pub mention_completion_idx: usize,
    pub modes: Vec<AcpSessionMode>,
    pub current_mode_id: Option<String>,
    pub usage: Option<AcpUsage>,
    pub auth_methods: Vec<AcpAuthMethod>,
    pub authenticated: bool,
    pub prompt_capabilities: AcpPromptCapabilities,
    pub tool_calls: Vec<AcpToolCall>,
    /// `agentCapabilities.mcpCapabilities` from this session's `initialize`
    /// response (#1487, redo of #1462 on the multi-session engine) — which
    /// optional MCP server transports (`http`/`sse`) the agent accepts,
    /// beyond the always-eligible `stdio`. Read by `Engine::
    /// acp_begin_session` to decide which of `settings.acp_mcp_servers`/
    /// `AcpAgentProfile::mcp_servers` to actually send on `session/new`/
    /// `session/load`.
    pub mcp_capabilities: AcpMcpCapabilities,
    /// Names of the MCP servers *this* session actually started with —
    /// i.e. what `session/new`/`session/load`'s `mcpServers` carried after
    /// `build_mcp_servers_wire` dropped anything the agent doesn't support
    /// (#1487). Empty before the session's handshake completes, or if none
    /// were configured/all were dropped. Drives the `:AiAgent` status
    /// line's `" | MCP: ..."` suffix (`acp_agent_registry_status_line`).
    pub active_mcp_servers: Vec<String>,

    /// This session's own transcript — `Engine::ai_messages` pre-#1463.
    pub ai_messages: Vec<AiMessage>,
    /// True while a turn on *this* session is in flight, regardless of
    /// which session is currently in the foreground.
    pub ai_streaming: bool,
}

impl AcpSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// True if this session has a request parked on
    /// `session/request_permission` awaiting a human decision — the
    /// signal a background tab badges (#1463's "surfaces permission
    /// prompts without stealing focus, e.g. with a badge").
    pub fn has_pending_permission(&self) -> bool {
        self.pending_permission.is_some()
    }

    /// True once this slot has ever been given content (a spawned client,
    /// or transcript messages) — used to decide whether `:AiNew` should
    /// reuse an entirely blank current session instead of piling up empty
    /// tabs.
    pub fn is_blank(&self) -> bool {
        self.client.is_none() && self.ai_messages.is_empty() && self.label.is_empty()
    }
}
