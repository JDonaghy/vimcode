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

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use super::acp::{
    AcpAuthMethod, AcpAvailableCommand, AcpChunkKind, AcpClient, AcpMcpCapabilities,
    AcpPermissionRequest, AcpPlanEntry, AcpPromptCapabilities, AcpSessionMode, AcpToolCall,
    AcpUsage,
};
use super::ai::AiMessage;

/// One cached markdown render: the source content's byte length, the
/// rendered [`quadraui::StyledText`], and its per-line heading scales. See
/// [`AcpSession::markdown_turn_cache`]'s doc.
pub type MarkdownTurnCacheEntry = (usize, quadraui::StyledText, Vec<f32>);

/// What one `ChatController` transcript turn index actually is (#1511) —
/// rebuilt fresh by `render::populate_ai_chat_controller` every frame into
/// [`AcpSession::transcript_turn_kinds`] and read back by
/// `Engine::dispatch_ai_chat_event`'s `TurnClicked` arm so a click/`Enter`
/// on a turn knows what to toggle (a tool-call card, a thought card) versus
/// what to leave alone (an ordinary message turn, the trailing plan
/// checklist).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptTurnKind {
    /// An ordinary `ai_messages[idx]` turn (user/assistant/thought-notice).
    Message(usize),
    /// A tool-call card, keyed by [`AcpToolCall::id`].
    ToolCall(String),
    /// The trailing plan-checklist turn (`AcpSession::plan`).
    Plan,
}

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
    /// #1512: a message submitted while this session already had a turn
    /// in flight (`ai_streaming`), instead of the silent no-op
    /// `Engine::ai_send_message` used to do in that case. Mirrors
    /// `pending_prompt`'s "hold the text, send it once the right event
    /// fires" shape, but a deliberately separate field rather than the
    /// same one — `pending_prompt` is consumed the moment the
    /// *handshake* finishes (`AcpEvent::SessionCreated`/`SessionLoaded`),
    /// while this is consumed the moment the *in-flight turn* finishes
    /// (`AcpEvent::PromptStopped`, `Engine::ai_dispatch_queued_message`)
    /// or immediately on "send now" (`Engine::ai_send_queued_now`,
    /// Ctrl+G — cancels the current turn first). The two triggers can
    /// never overlap in practice (queuing only ever starts once
    /// `ai_streaming` was already `true`, which implies the handshake
    /// already finished), but keeping them as separate fields avoids one
    /// consumer accidentally stealing the other's text.
    pub queued_prompt: Option<String>,
    /// Index into `ai_messages` of the dimmed "(queued)" turn
    /// `Engine::ai_queue_message` pushed for `queued_prompt` — flipped
    /// back to a plain `"user"` turn the moment the queued message is
    /// actually dispatched (`Engine::ai_dispatch_queued_message`), or
    /// relabelled "(discarded)" if the user drops it instead
    /// (`Engine::ai_discard_queued_message`, Ctrl+R).
    pub queued_prompt_idx: Option<usize>,
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
    /// Wall-clock start of the turn currently in flight (#1508). Stamped by
    /// `Engine::acp_begin_streaming` — the one place `ai_streaming` flips
    /// `true` from — so it can never drift out of sync with the flag it's
    /// paired with. `render::populate_ai_chat_controller`'s status-strip
    /// reads this to show an elapsed-time suffix (e.g. "· 12s") while busy.
    /// Left stale (not cleared) once the turn ends — harmless, since every
    /// reader gates on `ai_streaming` being `true` first.
    pub turn_started_at: Option<std::time::Instant>,

    /// Cache of markdown-rendered assistant/thought turns (#1510), keyed by
    /// index into `ai_messages`. Each entry pairs the source message's
    /// content length with the `quadraui::StyledText`/per-line heading-scale
    /// pair `quadraui::render_markdown_to_styled` produced for it.
    ///
    /// `render::populate_ai_chat_controller` rebuilds `ChatController`'s
    /// transcript from `ai_messages` on *every* frame (matching
    /// `set_transcript`'s "replace, don't accumulate" contract), but during
    /// a streaming turn only the newest (last) message's content actually
    /// changes frame-to-frame — every earlier, already-settled message would
    /// otherwise be re-parsed as markdown dozens of times a second for no
    /// reason. Keyed by content length rather than a hash of the content
    /// itself: a streamed message only ever grows (chunks are appended, not
    /// rewritten), so a length mismatch is a correct enough staleness check
    /// without hashing the whole string every frame.
    ///
    /// `Engine::ai_clear` empties this alongside `ai_messages` itself, so a
    /// cleared session's fresh messages never collide with stale entries
    /// left over at the same indices from the previous conversation.
    pub markdown_turn_cache: RefCell<HashMap<usize, MarkdownTurnCacheEntry>>,

    /// First-appearance anchor for each tool call (#1511): `ai_messages
    /// .len()` at the moment [`crate::core::engine::Engine::
    /// acp_upsert_tool_call`] first saw this id — i.e. "insert this card's
    /// transcript turn right after message index `anchor - 1`" (`anchor ==
    /// 0` means before the very first message). Recorded once per id and
    /// never touched by a later `tool_call_update` (an update patches an
    /// existing call in place; it never changes *when* the call happened).
    /// `render::populate_ai_chat_controller` reads this to interleave
    /// tool-call cards chronologically instead of appending every call
    /// after the whole conversation, which is the behaviour the issue this
    /// was added for (#1511) exists to fix.
    pub tool_call_anchor: HashMap<String, usize>,

    /// Tool-call cards the user has expanded (#1511) — every card starts
    /// collapsed; presence here means the user clicked/`Tab`+`Enter`-ed it
    /// open. Keyed by `AcpToolCall::id`, not a transcript index, since the
    /// chronological interleave (`tool_call_anchor` above) means a card's
    /// transcript index can shift frame to frame as earlier tool calls are
    /// upserted or messages stream in ahead of it.
    pub tool_call_expanded: HashSet<String>,

    /// Genuine thought turns (`AcpChunkKind::Thought`, see
    /// `render::is_genuine_thought_chunk`) the user has expanded (#1511,
    /// extending #1510's always-forced collapse). Every genuine thought
    /// turn starts collapsed as a one-line "Thinking…" card; presence here
    /// means the user opened it. Keyed by `ai_messages` index, the same key
    /// `markdown_turn_cache` uses — stable because a thought turn's index
    /// never moves once appended (only later messages append after it).
    pub thought_expanded: HashSet<usize>,

    /// Which `ChatController` transcript turn index is which, rebuilt fresh
    /// by `render::populate_ai_chat_controller` every frame (#1511) —
    /// read back by `Engine::dispatch_ai_chat_event`'s `TurnClicked` arm to
    /// resolve a click/`Enter` on a turn to "toggle this tool-call card" /
    /// "toggle this thought card" / "ordinary message turn, ignore".
    /// Meaningless before the first populate call of a frame, but
    /// `render::route_ai_chat_event` and both backends' `PANEL_AI` render
    /// arms always populate before `ChatController::handle` can produce a
    /// `TurnClicked`, so it is always fresh by the time a click is
    /// dispatched.
    pub transcript_turn_kinds: RefCell<Vec<TranscriptTurnKind>>,
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
