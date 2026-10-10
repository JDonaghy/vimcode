use super::*;

/// Adapt the quickfix panel data into a generic `quadraui::ListView`.
///
/// The quickfix panel is a simple flat list of pre-formatted strings
/// with a header. `ListView` maps one-to-one. No decoration per row
/// because the input strings don't carry severity info; future
/// enhancement: parse severity from the text or extend
/// `QuickfixPanel` to carry `Decoration`.
pub fn quickfix_to_list_view(qf: &QuickfixPanel) -> quadraui::ListView {
    use quadraui::{ListItem, ListView, StyledText, WidgetId};

    let focus_mark = if qf.has_focus { " [FOCUS]" } else { "" };
    let title_text = format!(" {} ({} items){}", qf.title, qf.total_items, focus_mark);

    let items: Vec<ListItem> = qf
        .items
        .iter()
        .map(|s| ListItem {
            text: StyledText::plain(s),
            icon: None,
            detail: None,
            decoration: quadraui::Decoration::Normal,
        })
        .collect();

    ListView {
        id: WidgetId::new("quickfix"),
        title: Some(StyledText::plain(title_text)),
        items,
        selected_idx: qf.selected_idx,
        scroll_offset: 0, // set by caller from local scroll_top
        has_focus: qf.has_focus,
        bordered: false,
        h_scroll: 0,
        max_content_width: None,
        show_v_scrollbar: false,
    }
}

/// Build [`BoardData`] from engine state (#521). Always builds so backends
/// can check `has_focus` even when there's nothing to paint yet.
pub(crate) fn build_board_data(engine: &Engine) -> Option<BoardData> {
    // A status banner is only shown in place of the board, never over it —
    // a stale-but-present model from an earlier successful fetch keeps
    // rendering even if the *next* refresh failed, so `board_error` only
    // becomes a banner when there is nothing else to show.
    let status = if engine.board_model.is_some() {
        None
    } else if let Some(err) = &engine.board_error {
        Some(err.clone())
    } else if engine.board_fetching {
        Some("Fetching board…".to_string())
    } else if engine.board_provider().is_none() {
        Some("No board provider configured".to_string())
    } else {
        Some("Fetching board…".to_string())
    };
    Some(BoardData {
        has_focus: engine.board_has_focus,
        model: engine.board_model.clone(),
        status,
    })
}

pub(crate) fn build_ext_panel_data(engine: &Engine) -> Option<ExtPanelData> {
    let panel_name = engine.ext_panel_active.as_ref()?;
    let reg = engine.ext_panels.get(panel_name)?;
    let expanded_vec = engine.ext_panel_sections_expanded.get(panel_name);
    let sections: Vec<ExtPanelSectionData> = reg
        .sections
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let expanded = expanded_vec.and_then(|v| v.get(i)).copied().unwrap_or(true);
            let key = (panel_name.clone(), name.clone());
            let all_items = engine
                .ext_panel_items
                .get(&key)
                .cloned()
                .unwrap_or_default();
            // Filter items for tree visibility (hide children of collapsed tree nodes)
            let visible_indices = engine.ext_panel_visible_indices(panel_name, &all_items);
            let items: Vec<_> = visible_indices
                .into_iter()
                .filter_map(|idx| {
                    all_items.get(idx).cloned().map(|mut item| {
                        // Resolve user-toggled tree expansion state into the
                        // item so `ext_panel_to_tree_view` doesn't need engine
                        // access. The engine map is the source of truth once
                        // the user has toggled; `item.expanded` is the plugin
                        // default otherwise.
                        if item.expandable {
                            let key = (panel_name.clone(), item.id.clone());
                            if let Some(&v) = engine.ext_panel_tree_expanded.get(&key) {
                                item.expanded = v;
                            }
                        }
                        item
                    })
                })
                .collect();
            ExtPanelSectionData {
                name: name.clone(),
                items,
                expanded,
            }
        })
        .collect();
    Some(ExtPanelData {
        name: panel_name.clone(),
        title: reg.title.clone(),
        sections,
        selected: engine.ext_panel_selected,
        has_focus: engine.ext_panel_has_focus,
        scroll_top: engine.ext_panel_scroll_top,
        input_text: engine
            .ext_panel_input_text
            .get(panel_name)
            .cloned()
            .unwrap_or_default(),
        input_active: engine.ext_panel_input_active,
        help_open: engine.ext_panel_help_open,
        help_bindings: engine
            .ext_panel_help_bindings
            .get(panel_name)
            .cloned()
            .unwrap_or_default(),
    })
}

/// True if `span` asks for any per-span presentation of its own — a
/// foreground/background colour, bold, italic, or underline — as opposed to
/// inheriting the row's role colour like ordinary body text does. Used by
/// [`markdown_turn_styled_cached`] to decide whether a rendered message has
/// anything worth handing to quadraui's styled (char-wrapped) transcript
/// path at all; see that function's doc.
fn span_carries_styling(span: &quadraui::StyledSpan) -> bool {
    span.fg.is_some() || span.bg.is_some() || span.bold || span.italic || span.underline
}

/// Render `content` as markdown into a single joined [`quadraui::StyledText`]
/// (`\n`-separated spans, one per source line, matching
/// [`quadraui::ChatController::push_turn_markdown`]'s own join) plus its
/// per-line heading scales, reusing `cache[idx]` when `content`'s length is
/// unchanged since the last call — see [`AcpSession::markdown_turn_cache`]'s
/// doc for why length is a sufficient staleness check here. Written as a
/// free function (rather than calling `push_turn_markdown` itself) because
/// [`populate_ai_chat_controller`] builds a whole `Vec<ChatTurn>` to hand to
/// `set_transcript` in one shot, not one turn at a time onto the live
/// controller.
///
/// # Plain content keeps the word-wrapped flat path
///
/// When the render comes back with *nothing to style* — every line at
/// scale `1.0` and every span carrying no fg/bg/bold/italic/underline,
/// i.e. the message contained no markdown at all — this returns the
/// rendered text as a single unstyled span and an **empty** `line_scales`.
/// That is not a micro-optimisation: `ChatController::
/// build_transcript_rows` picks its wrap policy off exactly that field —
/// turns with per-line scales wrap with `WrapPolicy::Char` (mid-word, at
/// the display-width budget), turns without them go down the flat path and
/// word-wrap via `text_util::word_wrap`. Handing quadraui styled rows for
/// a message that has no styling would therefore turn *every* plain agent
/// reply in a narrow AI panel into mid-word breaks (`"Hello world ANSWE"`
/// / `"RED1519"`) for zero rendering gain. Messages that genuinely carry
/// markdown still take the styled/char-wrapped path — word-wrapping those
/// is a quadraui-side follow-up (`ChatController`'s own doc flags the
/// policy switch at that call site as quadraui#821), not something vimcode
/// may fix with per-backend code here.
fn markdown_turn_styled_cached(
    cache: &std::cell::RefCell<
        std::collections::HashMap<usize, crate::core::acp_session::MarkdownTurnCacheEntry>,
    >,
    idx: usize,
    content: &str,
    theme: &quadraui::Theme,
) -> (quadraui::StyledText, Vec<f32>) {
    if let Some((cached_len, text, line_scales)) = cache.borrow().get(&idx) {
        if *cached_len == content.len() {
            return (text.clone(), line_scales.clone());
        }
    }
    let rendered = quadraui::render_markdown_to_styled(content, theme);
    let has_styling = rendered
        .line_scales
        .iter()
        .any(|s| (*s - 1.0).abs() > f32::EPSILON)
        || rendered
            .lines
            .iter()
            .any(|line| line.spans.iter().any(span_carries_styling));
    let mut spans: Vec<quadraui::StyledSpan> = Vec::new();
    for (i, line) in rendered.lines.into_iter().enumerate() {
        if i > 0 {
            spans.push(quadraui::StyledSpan::plain("\n"));
        }
        spans.extend(line.spans);
    }
    let text = quadraui::StyledText { spans };
    // See this function's "Plain content keeps the word-wrapped flat path"
    // doc section: empty `line_scales` is what selects word wrapping.
    let line_scales = if has_styling {
        rendered.line_scales
    } else {
        Vec::new()
    };
    cache
        .borrow_mut()
        .insert(idx, (content.len(), text.clone(), line_scales.clone()));
    (text, line_scales)
}

/// True if `ai_messages[idx]` (an `"assistant-thought"`-role turn) is
/// genuine agent reasoning (`AcpChunkKind::Thought`, streamed by
/// `Engine::acp_append_chunk`) rather than one of the one-shot system/error
/// notices *also* pushed under that same role string — agent-failed-to-
/// start, protocol-version-mismatch, turn-stopped-early, cancelled-by-user
/// (see `AiMessage`'s own doc on this pre-existing role conflation, and
/// `Engine::acp_cancel_turn`/`poll_acp`'s various pushes).
///
/// Genuine thought chunks are always followed, **within the same turn**, by
/// a real `"assistant"`-role reply; every notice case above is pushed as
/// (and then stays) the *last message of its turn* — nothing about a
/// terminal/error condition produces a further assistant reply before the
/// conversation's next user prompt. Crucially, this only scans forward
/// *up to the next `"user"`-role message* (exclusive) rather than the rest
/// of `ai_messages` — every real prompt turn starts with a fresh `"user"`
/// push (see `poll_acp`'s `SessionCreated`/`RequestFailed` arms and
/// `ai_send_message_via_acp`), so a later, unrelated turn's assistant reply
/// can never be mistaken for this turn's own reply. Review regression
/// (#1510): the previous "scan to the end of `ai_messages`" version let a
/// turn cancelled/stopped/failed mid-conversation get permanently
/// re-collapsed into "Thinking…" the moment the *next* prompt in the same
/// session got a normal reply — `Engine::acp_cancel_turn`/`PromptStopped`
/// (non-`end_turn`)/`RequestFailed` none tear down the session, so the
/// conversation carries on and that later reply used to satisfy the old,
/// unscoped "any later assistant message" check.
///
/// A thought chunk still streaming in with no reply yet (the brief window
/// between `agent_thought_chunk` and the first `agent_message_chunk`) reads
/// as `false` here too — shown in full rather than collapsed for that one
/// frame or two, self-correcting the moment the reply arrives within the
/// same turn. That false-negative is a far safer failure mode than the
/// reverse: permanently hiding a cancellation/error notice behind a
/// "Thinking..." summary.
pub(crate) fn is_genuine_thought_chunk(
    ai_messages: &[crate::core::ai::AiMessage],
    idx: usize,
) -> bool {
    let turn_end = ai_messages[idx + 1..]
        .iter()
        .position(|m| m.role == "user")
        .map(|offset| idx + 1 + offset)
        .unwrap_or(ai_messages.len());
    ai_messages[idx + 1..turn_end]
        .iter()
        .any(|m| m.role == "assistant")
}

/// Populate `engine.ai_chat` (a [`quadraui::ChatController`]) with the
/// current conversation and busy state for the next `render()`/`handle()`
/// pass — the AI-panel twin of [`populate_explorer_tree_controller`]. Call
/// once before either (both [`route_ai_chat_event`] and the two backends'
/// `PANEL_AI` render arms do).
///
/// `engine.acp().ai_messages` stays the business-logic source of truth (what
/// `crate::core::ai::send_chat` actually sends); this rebuilds the
/// controller's own `Vec<ChatTurn>` mirror from it on every call, matching
/// [`quadraui::ChatController::set_transcript`]'s "replace, don't
/// accumulate" contract so streamed/cleared history never double-renders.
pub fn populate_ai_chat_controller(
    engine: &Engine,
    theme: &Theme,
    backend: &dyn quadraui::Backend,
) {
    let user_fg = theme.keyword;
    // ACP-1 (#952): agent "thought" chunks (`session/update`'s
    // `agent_thought_chunk`, role "assistant-thought" — see
    // `Engine::acp_append_chunk`) render under `ChatRole::System`, not
    // `Assistant` — `ChatController::build_transcript_rows` gives `System`
    // both its own role-header label ("System" vs "AI") and its own colour,
    // which is the acceptance criterion: thought chunks must be visually
    // distinct from message chunks, not merely a different tint on the same
    // "AI" label.
    let thought_fg = theme.comment;
    // #1510: assistant/thought turns render as markdown
    // (`render_markdown_to_styled`) so fences, headings, lists, and inline
    // code paint as such instead of raw source text; user turns stay plain
    // (`StyledText::colored`) since a user's own words are never markdown
    // *content* to re-render, just text they typed. The per-role colour
    // that used to tint the whole message body is no longer the mechanism
    // that keeps thought turns visually distinct from assistant turns —
    // `ChatRole::System` (vs `Assistant`) already earns that on its own via
    // `ChatController::build_transcript_rows`'s role-header label/colour
    // (see the comment above), and thought turns additionally collapse to a
    // one-line summary below.
    let q_theme = to_quadraui_theme(theme);
    let acp = engine.acp();
    let ai_messages = &acp.ai_messages;

    // #1511: tool calls interleave chronologically instead of all landing
    // after the whole conversation (the pre-#1511 behaviour, and this
    // issue's core complaint — see `AcpSession::tool_call_anchor`'s doc).
    // Bucket every call by its recorded anchor once, up front, so the
    // per-index interleave loop below is a plain lookup rather than an
    // O(messages × tool_calls) scan. `anchor` ranges `0..=ai_messages.len()`
    // by construction (`Engine::acp_upsert_tool_call` stamps it from
    // `ai_messages.len()` at the moment the call first appeared), so a
    // `Vec` indexed directly by anchor needs no `HashMap` at all.
    let mut tool_calls_by_anchor: Vec<Vec<&crate::core::acp::AcpToolCall>> =
        vec![Vec::new(); ai_messages.len() + 1];
    for call in &acp.tool_calls {
        let anchor = acp
            .tool_call_anchor
            .get(&call.id)
            .copied()
            .unwrap_or(ai_messages.len())
            .min(ai_messages.len());
        tool_calls_by_anchor[anchor].push(call);
    }

    let mut turns: Vec<quadraui::ChatTurn> = Vec::new();
    let mut kinds: Vec<crate::core::acp_session::TranscriptTurnKind> = Vec::new();

    // #955 (ACP-4) / #1511: one card per tool call — collapsed by default
    // (`turn_summary_line` below is `tool_call_title_line`'s single-line
    // `{glyph} {kind}: {title}`), expanded on demand into the full body
    // `tool_call_expanded_text` builds (locations, content blocks,
    // rawInput/rawOutput). `set_turn_collapsed`/`set_turn_summary` calls
    // for these land after `set_transcript` below, once `kinds` (built
    // here) is known to line up with the transcript it describes.
    let push_tool_calls_at =
        |anchor: usize,
         turns: &mut Vec<quadraui::ChatTurn>,
         kinds: &mut Vec<crate::core::acp_session::TranscriptTurnKind>| {
            for call in &tool_calls_by_anchor[anchor] {
                let expanded = acp.tool_call_expanded.contains(&call.id);
                // Deliberately plain (`StyledText::colored`), never markdown
                // — unlike assistant/thought turns (#1510). Routing this
                // through `render_markdown_to_styled` would flip the whole
                // card onto quadraui's char-wrapped `WrapPolicy::Char` path
                // the moment *any* line in it carries styling (the fenced
                // ```json``` blocks always do), which would mid-word-break
                // the plain `-> path:line` header/location lines too —
                // exactly the "Hello world ANSWE"/"RED1519" failure mode
                // `markdown_turn_styled_cached`'s own doc warns about,
                // just triggered by this card's own content instead of an
                // unrelated later message. Plain text keeps the whole card
                // on the word-wrapped flat path, same as before #1511.
                let text = if expanded {
                    quadraui::StyledText::colored(
                        crate::core::acp::tool_call_expanded_text(call, |terminal_id| {
                            acp.acp_terminals
                                .get(terminal_id)
                                .map(|record| record.view())
                        }),
                        thought_fg,
                    )
                } else {
                    // Collapsed cards render through `turn_summary_line`
                    // instead (a single `MessageRow`, set below) — this
                    // `ChatTurn::text` value is never painted while
                    // collapsed, so an empty placeholder is fine here.
                    quadraui::StyledText::plain("")
                };
                turns.push(quadraui::ChatTurn {
                    role: quadraui::ChatRole::System,
                    text,
                    timestamp_unix: None,
                    line_scales: Vec::new(),
                });
                kinds.push(crate::core::acp_session::TranscriptTurnKind::ToolCall(
                    call.id.clone(),
                ));
            }
        };

    for (idx, m) in ai_messages.iter().enumerate() {
        push_tool_calls_at(idx, &mut turns, &mut kinds);
        let chat_turn = match m.role.as_str() {
            "user" => quadraui::ChatTurn {
                role: quadraui::ChatRole::User,
                text: quadraui::StyledText::colored(m.content.clone(), user_fg),
                timestamp_unix: None,
                line_scales: Vec::new(),
            },
            // #1512: a message submitted while a turn was already in
            // flight — queued rather than dropped (`Engine::
            // ai_queue_message`). Dimmed (`thought_fg`, the same tone
            // `assistant-thought` turns use) and literally prefixed
            // `"(queued) "` on the painted text, not just a colour: a
            // colour alone isn't something a black-box test (or a
            // low-color terminal) can reliably read back, unlike a
            // substring. Flips back to plain `"user"` the moment it's
            // actually sent (`Engine::ai_dispatch_queued_message`), or
            // gets a `" (discarded)"` suffix if dropped instead
            // (`Engine::ai_discard_queued_message`) while staying on this
            // same dimmed role — it never became a real turn either way.
            "user-queued" => quadraui::ChatTurn {
                role: quadraui::ChatRole::User,
                text: quadraui::StyledText::colored(format!("(queued) {}", m.content), thought_fg),
                timestamp_unix: None,
                line_scales: Vec::new(),
            },
            "assistant-thought" => {
                let (text, line_scales) = markdown_turn_styled_cached(
                    &acp.markdown_turn_cache,
                    idx,
                    &m.content,
                    &q_theme,
                );
                quadraui::ChatTurn {
                    role: quadraui::ChatRole::System,
                    text,
                    timestamp_unix: None,
                    line_scales,
                }
            }
            _ => {
                let (text, line_scales) = markdown_turn_styled_cached(
                    &acp.markdown_turn_cache,
                    idx,
                    &m.content,
                    &q_theme,
                );
                quadraui::ChatTurn {
                    role: quadraui::ChatRole::Assistant,
                    text,
                    timestamp_unix: None,
                    line_scales,
                }
            }
        };
        turns.push(chat_turn);
        kinds.push(crate::core::acp_session::TranscriptTurnKind::Message(idx));
    }
    // Trailing tool calls anchored at `ai_messages.len()` — issued after the
    // most recent message currently in the transcript (the common case
    // while a turn is still streaming: message chunks lag the tool calls
    // that triggered them).
    push_tool_calls_at(ai_messages.len(), &mut turns, &mut kinds);

    // #956/#1513: the agent's current plan no longer appends as a synthetic
    // trailing transcript turn — see `ai_plan_multi_section_view`'s doc for
    // why (streaming text used to push it around and scroll it away, #1513's
    // whole complaint). It paints as its own pinned block above the
    // transcript instead; `ai_messages`/`turns`/`kinds` here are untouched
    // by `engine.acp().plan`.
    *acp.transcript_turn_kinds.borrow_mut() = kinds;

    let mut chat = engine.ai_chat.borrow_mut();
    chat.set_transcript(turns);
    // #1510/#1511: thought turns and tool-call cards each collapse to a
    // one-line summary by default; a user click/`Tab`+`Enter` toggle
    // (`Engine::dispatch_ai_chat_event`'s `TurnClicked`/`KeyPressed` arms)
    // records the override in `AcpSession::thought_expanded`/
    // `tool_call_expanded`, keyed by `ai_messages` index / tool-call id
    // respectively rather than transcript index — the interleave above
    // means a card's transcript index can move frame to frame as earlier
    // tool calls upsert or new messages stream in ahead of it, but its
    // `ai_messages` index / call id never does.
    //
    // Written for every index in the *current* transcript on every frame,
    // the "stays collapsed"/`None` case included, because `ChatController`
    // keeps collapsed/summary state in its own maps **keyed by transcript
    // index**, and `set_transcript` deliberately does not clear them (see
    // its doc). Only setting the "expanded" case would therefore leak a
    // previous conversation's collapse onto whatever later lands at the
    // same index: after `:AiClear` + `:AiSessions` resume, index 1 was a
    // thought turn before the clear and the replayed *assistant* reply
    // after it, so the reply painted as a collapsed "Thinking…" card with
    // its text nowhere on screen — the bug
    // `ai_sessions_picker_resumes_a_past_session_and_rebuilds_the_
    // transcript` catches.
    for (turn_idx, kind) in acp.transcript_turn_kinds.borrow().iter().enumerate() {
        use crate::core::acp_session::TranscriptTurnKind;
        match kind {
            TranscriptTurnKind::Message(idx) => {
                // Gated on `is_genuine_thought_chunk`, not just `role ==
                // "assistant-thought"`: that role string is pre-existing
                // shorthand ACP-1 reused for two different things (see
                // `AiMessage`'s own doc) — real `agent_thought_chunk`
                // reasoning, *and* one-shot system/error notices
                // (`Engine::acp_cancel_turn`'s `"[cancelled by user]"`,
                // `poll_acp`'s failed-request/protocol-mismatch warnings,
                // ...). Collapsing those into "Thinking..." would hide the
                // one thing the user most needs to see right after the
                // turn ended abnormally.
                let is_thought = ai_messages.get(*idx).is_some_and(|m| {
                    m.role == "assistant-thought" && is_genuine_thought_chunk(ai_messages, *idx)
                });
                let collapsed = is_thought && !acp.thought_expanded.contains(idx);
                chat.set_turn_collapsed(turn_idx, collapsed);
                chat.set_turn_summary(turn_idx, collapsed.then(|| "Thinking\u{2026}".to_string()));
            }
            TranscriptTurnKind::ToolCall(id) => {
                let call = acp.tool_calls.iter().find(|c| &c.id == id);
                let collapsed = !acp.tool_call_expanded.contains(id);
                chat.set_turn_collapsed(turn_idx, collapsed);
                chat.set_turn_summary(
                    turn_idx,
                    collapsed
                        .then(|| call.map(crate::core::acp::tool_call_title_line))
                        .flatten(),
                );
            }
        }
    }
    chat.set_busy(engine.acp().ai_streaming);
    // #1509: plumb the `ai_chat_submit_on_enter` setting straight through to
    // `ChatController` every call — cheap enough to set unconditionally
    // (`ChatController::set_submit_on_enter` is a plain field write) and
    // this is the one place both backends already funnel through before
    // every `render()`/`handle()` pass, so a `:set` toggle takes effect on
    // the very next frame without either backend needing its own wiring.
    chat.set_submit_on_enter(engine.settings.ai_chat_submit_on_enter);
    // #1508: advance the status-strip's `Spinner` primitive to the frame
    // `Engine::tick_ai_spinner` last stamped (`poll_idle`, ≥4 Hz while busy
    // — see `App::tick_dispatch`'s `request_frame_in` re-arm). Harmless to
    // call every frame regardless of `busy`: `ChatController::render` only
    // ever paints the glyph when `self.busy` is also true (see
    // `set_busy`'s own doc), so this can't animate a frozen/idle panel.
    chat.set_spinner_frame(engine.ai_spinner_frame);
    let header_fg = theme.status_fg;
    let ai_chat_icon = crate::icons::AI_CHAT.nerd;
    let mut header = if engine.acp().ai_streaming {
        format!(
            " {ai_chat_icon} AI ASSISTANT  {}",
            ai_busy_status_text(engine)
        )
    } else {
        format!(" {ai_chat_icon} AI ASSISTANT")
    };
    // #1463: the session tab strip. Folded into this same always-repainted
    // header line — exactly like the mode/usage suffix below — rather than
    // a bespoke widget in either backend, per the Platform-Neutrality Rule:
    // both backends already paint this one string with zero backend-
    // specific code, so a second session is "more text in the string
    // that's already shared", not new GTK/TUI paint logic. Only shown once
    // a second tab actually exists — the common single-session case looks
    // exactly as it did before #1463. The active tab is marked `*`; a tab
    // with a `session/request_permission` parked while backgrounded (see
    // `Engine::acp_handle_permission_request`'s doc) is marked `!` — the
    // badge #1463 asks for, painted, not just an engine flag (`Engine::
    // acp_session_tabs` is the one source both this and the flag read).
    let tabs = engine.acp_session_tabs();
    if tabs.len() > 1 {
        let strip: Vec<String> = tabs
            .iter()
            .map(|(_, label, is_active, badged)| {
                let mark = if *is_active { "*" } else { "" };
                let badge = if *badged { "!" } else { "" };
                format!("{mark}{label}{badge}")
            })
            .collect();
        header.push_str(&format!("  [{}]", strip.join(" | ")));
    }
    // #1513: "Plan n/m" progress folds into this same always-repainted
    // header line — the pinned plan block below (`ai_plan_multi_section_
    // view`) carries the full checklist, but a quick "how far along" glance
    // shouldn't require expanding it, same reasoning as every other chip/
    // suffix on this line.
    if !acp.plan.is_empty() {
        let total = acp.plan.len();
        let done = acp
            .plan
            .iter()
            .filter(|e| e.status == crate::core::acp::AcpPlanEntryStatus::Completed)
            .count();
        header.push_str(&format!("  \u{b7} Plan {done}/{total}"));
    }
    // #956 (ACP-5): current mode + usage telemetry both fold into this one
    // existing status line rather than a new widget — "unobtrusive status
    // indicator" per the issue, and by construction can't steal focus or
    // churn layout since the header is already repainted every frame at a
    // fixed position.
    // #1520: `configOptions`' `category: "model"` entry is ACP v1's only
    // model picker (no dedicated `session/set_mode`-style method for a
    // model) — prefer showing it here over the older `modes` segment
    // below when both are present, rather than showing both for the same
    // underlying "what is this agent currently set to" question.
    if let Some(model_option) = engine
        .acp()
        .config_options
        .iter()
        .find(|o| o.category.as_deref() == Some("model"))
    {
        header.push_str(&format!(
            "  \u{b7} model: {}",
            model_option.current_value_label()
        ));
    } else if let Some(mode_id) = engine.acp().current_mode_id.as_deref() {
        let mode_label = engine
            .acp()
            .modes
            .iter()
            .find(|m| m.id == mode_id)
            .map(|m| m.name.as_str())
            .unwrap_or(mode_id);
        header.push_str(&format!("  \u{b7} mode: {mode_label}"));
    }
    if let Some(usage) = &engine.acp().usage {
        let summary = crate::core::acp::format_usage_summary(usage);
        if !summary.is_empty() {
            header.push_str(&format!("  \u{b7} {summary}"));
        }
    }
    // #1450 point 4: a staged Visual-selection/`:{range}AI` attachment's
    // `⧉`-chip rides on this same always-repainted, focus-safe status line
    // — visible until it's sent (or removed with Ctrl+R,
    // `Engine::dispatch_ai_chat_event`) — rather than a new widget, same
    // reasoning as the mode/usage lines above it.
    if let Some(attachment) = &engine.acp_pending_attachment {
        header.push_str(&format!(
            "  \u{b7} {}",
            attachment.chip(&engine.acp_workspace_cwd())
        ));
    }
    // #1464: every manually attached file/image gets its own chip on this
    // same always-repainted, focus-safe status line — same reasoning as
    // the range attachment's chip immediately above, extended to a list
    // since more than one file can be attached at once.
    for attachment in &engine.acp_manual_attachments {
        header.push_str(&format!("  \u{b7} {}", attachment.chip()));
    }
    // #1513: every staged `@symbol` mention gets its own chip on this same
    // always-repainted, focus-safe status line — same reasoning as the
    // manual-attachment chips immediately above.
    for symbol in &engine.acp_pending_symbol_mentions {
        header.push_str(&format!(
            "  \u{b7} {}",
            symbol.chip(&engine.acp_workspace_cwd())
        ));
    }
    // #1512: "queued (1)" while a message submitted mid-turn is waiting to
    // be sent (`AcpSession::queued_prompt`) — same always-repainted,
    // focus-safe status line as every other chip/suffix here. Only ever
    // one slot (`Engine::ai_queue_message` replaces rather than stacks),
    // so the count is always literally `1` while this segment shows at
    // all.
    if engine.acp().queued_prompt.is_some() {
        header.push_str("  \u{b7} queued (1)");
    }
    // #1515: `badge` mode's "Edited N files · +a -r" segment — the
    // replacement for the pre-#1515 always-auto-open full-viewport turn
    // review (`acp_review_on_turn_end` setting, `Engine::acp_end_turn`).
    // Folds into this same always-repainted, focus-safe status line, same
    // reasoning as every other chip/suffix above it. `:AiReview`
    // (`Engine::cmd_ai_review`) opens the same modal this segment
    // summarises.
    if engine.settings.acp_review_on_turn_end == crate::core::settings::AcpReviewOnTurnEnd::Badge {
        if let Some((files, added, removed)) = engine.acp_turn_review_badge() {
            header.push_str(&format!(
                "  \u{b7} Edited {files} file{} \u{b7} +{added} -{removed} (:AiReview)",
                if files == 1 { "" } else { "s" }
            ));
        }
    }
    chat.set_status(quadraui::StyledText::colored(header, header_fg));
    chat.set_hint(Some(quadraui::StyledText::colored(
        ai_chat_hint_line(
            backend,
            engine.settings.ai_chat_submit_on_enter,
            engine.acp().queued_prompt.is_some(),
        ),
        theme.comment,
    )));
}

// ── #1513: pinned, collapsible plan block ────────────────────────────────

/// Build the `quadraui::MultiSectionView` for the AI panel's pinned plan
/// block, or `None` while `engine.acp().plan` is empty (nothing to pin).
///
/// One section, not the whole `ChatController` transcript: pre-#1513 the
/// plan rendered as a synthetic *trailing* transcript turn
/// (`populate_ai_chat_controller` used to push one, see that function's git
/// history) — streaming assistant text kept appending turns after it, so
/// the plan drifted down and off the visible viewport the moment a reply
/// started streaming, exactly the issue's complaint. Pinning it in its own
/// band above `ChatController` (painted by `paint_ai_plan_band`, in a rect
/// carved out of the panel *before* `ChatController::render` ever sees the
/// remainder) means it can never move regardless of how much transcript
/// text streams in below it.
///
/// Collapsed (the default, `!engine.ai_plan_expanded`) shows only the
/// header: `"Plan n/m: <in-progress entry>"` (or "All steps complete" once
/// `n == m`) — enough to see what the agent is doing right now without
/// spending any vertical space on the rest of the list. Expanded shows the
/// full checklist as the section body, one line per entry with the same
/// status glyphs `plan_to_checklist_text` uses, so a reader who already
/// knows that convention doesn't have to learn a second one.
pub fn ai_plan_multi_section_view(
    engine: &Engine,
    theme: &Theme,
) -> Option<quadraui::MultiSectionView> {
    let plan = &engine.acp().plan;
    if plan.is_empty() {
        return None;
    }
    let total = plan.len();
    let done = plan
        .iter()
        .filter(|e| e.status == crate::core::acp::AcpPlanEntryStatus::Completed)
        .count();
    let expanded = engine.ai_plan_expanded;
    let title = if expanded {
        format!("Plan {done}/{total}")
    } else {
        let current = plan
            .iter()
            .find(|e| e.status == crate::core::acp::AcpPlanEntryStatus::InProgress)
            .map(|e| e.content.clone())
            .unwrap_or_else(|| {
                if done == total {
                    "All steps complete".to_string()
                } else {
                    "Plan".to_string()
                }
            });
        format!("Plan {done}/{total}: {current}")
    };
    let lines: Vec<quadraui::StyledText> = plan
        .iter()
        .map(|e| {
            let (glyph, color) = match e.status {
                crate::core::acp::AcpPlanEntryStatus::Completed => ('\u{2611}', theme.comment),
                crate::core::acp::AcpPlanEntryStatus::InProgress => ('\u{25d0}', theme.keyword),
                crate::core::acp::AcpPlanEntryStatus::Pending => ('\u{2610}', theme.foreground),
            };
            quadraui::StyledText::colored(format!("{glyph} {}", e.content), color)
        })
        .collect();
    Some(quadraui::MultiSectionView {
        id: quadraui::WidgetId::new("ai-plan"),
        sections: vec![quadraui::Section {
            id: "plan".to_string(),
            header: quadraui::SectionHeader {
                icon: None,
                title: quadraui::StyledText::plain(title),
                badge: None,
                actions: Vec::new(),
                show_chevron: true,
            },
            body: quadraui::SectionBody::Text(lines),
            aux: None,
            size: quadraui::SectionSize::EqualShare,
            collapsed: !expanded,
            min_size: None,
            max_size: None,
        }],
        active_section: None,
        axis: quadraui::MsvAxis::Vertical,
        allow_resize: false,
        allow_collapse: true,
        scroll_mode: quadraui::ScrollMode::WholePanel,
        has_focus: false,
        panel_scroll: 0.0,
    })
}

/// Height (in `unit_h` units — pixels on GTK, cells on TUI, same convention
/// every other AI-panel geometry helper here uses) the plan band should
/// reserve at the top of the AI panel rect this frame: `0.0` when there is
/// no plan to pin, one row for the header when collapsed, or the header
/// plus one row per entry when expanded. Callers subtract this from the
/// panel rect's height *before* handing the remainder to `ChatController`
/// (`paint_ai_plan_band`'s doc has the call sequence).
pub fn ai_plan_band_height(engine: &Engine, unit_h: f32) -> f32 {
    let Some(plan_len) = (!engine.acp().plan.is_empty()).then(|| engine.acp().plan.len()) else {
        return 0.0;
    };
    let header_rows = 1.0;
    let body_rows = if engine.ai_plan_expanded {
        plan_len as f32
    } else {
        0.0
    };
    (header_rows + body_rows) * unit_h.max(1.0)
}

/// Paint the plan block into `rect` (the band `ai_plan_band_height` sized)
/// and cache the layout it painted with in `Engine::ai_plan_layout` — same
/// "cache what actually got painted" contract `ai_chat_rect`/
/// `ext_panel_tree_layout` already use, so `route_ai_plan_band_click`'s
/// hit-test can never derive a different geometry than what's on screen.
/// No-op (and clears the cached layout/rect) when there's no plan to pin.
pub fn paint_ai_plan_band(
    backend: &mut dyn quadraui::Backend,
    engine: &Engine,
    theme: &Theme,
    rect: quadraui::Rect,
) {
    let Some(view) = ai_plan_multi_section_view(engine, theme) else {
        engine
            .ai_plan_rect
            .set(quadraui::Rect::new(0.0, 0.0, 0.0, 0.0));
        *engine.ai_plan_layout.borrow_mut() = None;
        return;
    };
    if rect.width <= 0.0 || rect.height <= 0.0 {
        engine
            .ai_plan_rect
            .set(quadraui::Rect::new(0.0, 0.0, 0.0, 0.0));
        *engine.ai_plan_layout.borrow_mut() = None;
        return;
    }
    engine.ai_plan_rect.set(rect);
    let unit_h = backend.line_height().max(1.0);
    let metrics = quadraui::MsvLayoutMetrics {
        header_size: unit_h,
        divider_size: 0.0,
        scrollbar_size: 0.0,
        cell_quantum: 0.0,
    };
    // Body content size in main-axis units: one row per plan entry — the
    // only section, so `SectionSize::EqualShare` above already consumes
    // exactly `rect`'s height regardless of this value, but `layout` still
    // wants a `measure` closure per its signature.
    let plan_len = engine.acp().plan.len() as f32;
    let layout = view.layout(rect, metrics, |_| quadraui::SectionMeasure {
        content_size: plan_len * unit_h,
        aux_size: 0.0,
    });
    backend.draw_multi_section_view(rect, &view);
    *engine.ai_plan_layout.borrow_mut() = Some(layout);
}

/// Route a press at `pos` (same coordinate space `ai_plan_rect`/
/// `ai_plan_layout` were painted in) against the plan block. Only the
/// header is interactive — clicking it (anywhere in the title area, or the
/// chevron) toggles `Engine::ai_plan_expanded`. Returns whether the press
/// landed on the band at all, so the caller (`App::route_ai_sidebar_event`)
/// knows whether to forward the event to `ChatController` instead.
pub fn route_ai_plan_band_click(engine: &mut Engine, pos: quadraui::Point) -> bool {
    let layout = engine.ai_plan_layout.borrow();
    let Some(layout) = layout.as_ref() else {
        return false;
    };
    match layout.hit_test(pos.x, pos.y) {
        quadraui::MultiSectionViewHit::Header { .. } => {
            engine.ai_plan_expanded = !engine.ai_plan_expanded;
            true
        }
        quadraui::MultiSectionViewHit::Outside => false,
        // Body/divider/scrollbar/inert: still inside the band's bounds —
        // consumed so a click there doesn't fall through to the editor
        // underneath, but nothing to toggle.
        _ => true,
    }
}

/// Braille spinner frame table (#1508) — the same 10-glyph rotation
/// quadraui's own GTK/TUI/macOS `Spinner` rasterisers use internally
/// (`gtk::spinner`/`macos::spinner`'s private `FRAMES` const), duplicated
/// here because neither is a public quadraui export: this one is embedded
/// directly into the status-strip *text* (`⠹ execute: cargo test · 12s`),
/// not painted through the `Spinner` primitive itself (that's the
/// separate icon `ChatController::set_spinner_frame`, just above, already
/// drives). A cosmetic text constant shared verbatim by both backends —
/// not per-backend rendering logic — so duplicating it here doesn't need
/// a quadraui issue first.
const SPINNER_GLYPHS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn spinner_glyph(frame_idx: usize) -> char {
    SPINNER_GLYPHS[frame_idx % SPINNER_GLYPHS.len()]
}

/// The AI-panel status strip's busy-state text (#1508) — what used to be
/// the frozen literal `"(thinking\u{2026})"` — built fresh every
/// `populate_ai_chat_controller` call so it reflects the current instant.
/// Only ever called while `engine.acp().ai_streaming` is true (see the one
/// call site).
///
/// Precedence, most specific first:
/// 1. A `session/request_permission` parked on the *active* session
///    (`AcpSession::has_pending_permission`) — the turn is not
///    progressing at all until a human answers, so naming the one
///    in-progress tool call here would be misleading (that call is the
///    one the permission is blocking, but nothing about it is actually
///    executing right now).
/// 2. The most recently announced still-running tool call (searched from
///    the end of `tool_calls` — an upserted `Vec`, not append-order for
///    updates — so a later call's `in_progress` status wins over an
///    earlier one still shown as `pending`/`in_progress` from a prior
///    step of the same turn).
/// 3. A generic "thinking" fallback — covers both the direct-curl
///    transport (which never populates `tool_calls` at all) and the ACP
///    gap between "turn started" and "first tool_call announced".
///
/// Every branch is prefixed with the same animated glyph and suffixed
/// with the same elapsed-time reading, so the busy indicator always
/// carries both pieces of information the issue asks for regardless of
/// which branch fires.
fn ai_busy_status_text(engine: &Engine) -> String {
    let glyph = spinner_glyph(engine.ai_spinner_frame);
    let elapsed = engine
        .acp()
        .turn_started_at
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    if engine.acp().has_pending_permission() {
        return format!("{glyph} Awaiting permission \u{b7} {elapsed}s");
    }
    let running_call = engine
        .acp()
        .tool_calls
        .iter()
        .rev()
        .find(|c| c.status == crate::core::acp::AcpToolCallStatus::InProgress);
    match running_call {
        Some(call) => format!("{glyph} {}: {} \u{b7} {elapsed}s", call.kind, call.title),
        None => format!("{glyph} thinking\u{2026} \u{b7} {elapsed}s"),
    }
}

/// The persistent one-line hint `populate_ai_chat_controller` pins to
/// [`quadraui::ChatController::set_hint`] (#1507) — unlike the built-in
/// `TextInput` placeholder it replaces, this stays visible once the user
/// starts typing (`ChatController`'s own *Persistent hint line* doc), which
/// is the whole point: the send/stop/leave keys were previously only
/// discoverable in the empty-input state.
///
/// The send-key list is adapted to what actually works on the live
/// backend rather than a single hardcoded string:
///
/// - GTK (`backend_caps().generic_font_families` — true only for a real
///   font-resolving GUI backend, `false` on every terminal backend per
///   that field's own doc) reliably delivers a distinct `Ctrl+Enter`, so
///   that's the one binding shown.
/// - TUI always has `Alt+Enter` and `Ctrl+S` (`ChatController::handle`'s
///   module doc: `Ctrl+S` "works on all terminals in both submit_on_enter
///   modes"). `Ctrl+Enter` is only added when `backend_caps().
///   kitty_keyboard` is actually active — that field's own doc says
///   exactly this: "check this before relying on a gesture that needs the
///   protocol (e.g. Ctrl+Enter distinct from Enter) and fall back to an
///   always-available binding (Alt+Enter) when it's false" — so a hint
///   promising a chord the detected terminal can't deliver never ships.
///
/// `Esc` and `<leader>ai` both return focus to the editor — the latter
/// toggles when the panel already has focus and its input is empty (see
/// [`Engine::ai_leader_toggle_key`], called from this same
/// `route_ai_chat_event`) — so both are named rather than just `Esc` alone,
/// closing the discoverability gap #1507 reports for the focus toggle too.
///
/// `submit_on_enter` (#1509, `Settings::ai_chat_submit_on_enter`) flips which
/// key does what, per [`quadraui::ChatController`]'s own *Keyboard
/// behaviour* doc: when `true` (the default, Zed parity) plain `Enter` sends
/// and `Shift+Enter`/`Alt+Enter` insert a newline instead; when `false`
/// (the pre-#1509 default) `Enter` always inserts a newline and the
/// `send_keys` chord below sends. Either way `Ctrl+S`/`Ctrl+Enter` (when
/// available) still send, matching `ChatController::handle`'s "in both
/// modes" bindings — but the hint only needs to name the *primary* gesture
/// for whichever mode is active, not every equivalent chord.
fn ai_chat_hint_line(
    backend: &dyn quadraui::Backend,
    submit_on_enter: bool,
    // #1512: whether a message is currently queued
    // (`AcpSession::queued_prompt`) — when it is, the hint grows the
    // `^G send now \u{b7} ^R discard` pair so the two new gestures are
    // discoverable exactly while they're actually relevant, rather than
    // permanently lengthening this line for every AI-panel frame.
    has_queued: bool,
) -> String {
    let caps = backend.backend_caps();
    let send_keys = if caps.generic_font_families {
        "Ctrl+Enter".to_string()
    } else if caps.kitty_keyboard {
        "\u{2325}\u{23ce}/^S/Ctrl+Enter".to_string()
    } else {
        "\u{2325}\u{23ce}/^S".to_string()
    };
    // `<leader>ai` is shown verbatim regardless of `Settings::leader`'s
    // actual character — the same convention every other leader-sequence
    // hint in this codebase uses (e.g. `mod.rs`'s command-palette
    // `shortcut: "<leader>sw"` fields), not a literal re-expansion.
    let entry_line = if submit_on_enter {
        "\u{23ce} send \u{b7} \u{21e7}\u{23ce} newline".to_string()
    } else {
        format!("\u{23ce} newline \u{b7} {send_keys} send")
    };
    let queued_hint = if has_queued {
        " \u{b7} ^G send now \u{b7} ^R discard"
    } else {
        ""
    };
    format!("{entry_line} \u{b7} ^C stop{queued_hint} \u{b7} Esc/<leader>ai editor")
}

/// Paint the slash-command or `@`-mention completion popup above the AI
/// panel's input box, whichever [`Engine::ai_command_completions`] /
/// [`Engine::ai_mention_completions`] (#1449) currently has a match — the
/// two are mutually exclusive by construction (see
/// `route_ai_chat_event`'s intercept). Reuses
/// [`completion_menu_to_quadraui_completions`] and the
/// `quadraui::Completions` primitive verbatim — the same machinery the
/// editor's own word-completion popup uses — rather than a bespoke widget.
///
/// `chat_rect` must be the same rect the caller's last `ai_chat.render()`
/// call used (`Engine::ai_chat_rect`), matching every other AI-panel
/// helper's contract. Anchoring at the rect's bottom edge with the rect
/// itself as the viewport makes `Completions::layout`'s own "prefer below,
/// flip above on overflow" placement logic put the popup just above the
/// panel's bottom edge — where `ChatController`'s input box always is —
/// without this function needing to know that box's exact pixel/cell
/// geometry (`ChatController` doesn't expose it).
pub fn paint_ai_command_completions(
    backend: &mut dyn quadraui::Backend,
    engine: &Engine,
    chat_rect: quadraui::Rect,
) {
    if chat_rect.width <= 0.0 || chat_rect.height <= 0.0 {
        return;
    }
    let Some(menu) = engine
        .ai_command_completions()
        .or_else(|| engine.ai_mention_completions())
    else {
        return;
    };
    let completions = completion_menu_to_quadraui_completions(&menu);
    let unit_h = backend.line_height().max(1.0);
    let popup_width = chat_rect.width.max(unit_h * 4.0);
    let max_popup_height = (unit_h * (menu.candidates.len() as f32 + 1.0)).min(chat_rect.height);
    let layout = completions.layout(
        chat_rect.x,
        chat_rect.y + chat_rect.height,
        unit_h,
        chat_rect,
        popup_width,
        max_popup_height,
        |_| quadraui::CompletionItemMeasure::new(unit_h),
    );
    backend.draw_completions(&completions, &layout);
}

/// Build the cell grid for a single terminal session.
///
/// Uses `TerminalSession::to_terminal()` to get the base snapshot from the
/// quadraui primitive. Post-processes in place: clears `is_cursor` when
/// `cursor_active` is `false`, and stamps find-match highlights.
///
/// `find` is `(matches, qlen, active_match_idx)`.
#[allow(clippy::type_complexity)]
fn build_pane_rows(
    sess: &quadraui::terminal_engine::TerminalSession,
    cursor_active: bool,
    find: Option<(&[(usize, u16, u16)], usize, usize)>,
) -> Vec<Vec<quadraui::TerminalCell>> {
    // Build snapshot — quadraui handles history blending, scroll offset, selection, cursor.
    // The WidgetId is a placeholder; only `snapshot.cells` is used here — the Terminal
    // struct is immediately destructured and the id is discarded.  The scrollbar is
    // built separately in `build_terminal_draw_data` from `TerminalPanel` fields.
    let snapshot = sess.to_terminal(quadraui::WidgetId::new("_pane"), None);
    let scroll_offset = sess.scroll_offset();
    let rows_count = snapshot.cells.len();

    let mut rows = snapshot.cells;

    // Clear cursor marker when this pane is not the focused one.
    if !cursor_active {
        for row in &mut rows {
            for cell in row {
                cell.is_cursor = false;
            }
        }
    }

    // Apply find-match highlights.
    if let Some((matches, qlen, active_idx)) = find {
        let current_offset = scroll_offset as isize;
        let term_rows = rows_count as isize;
        for (mi, &(moffset, mr, mc)) in matches.iter().enumerate() {
            let visible_row = mr as isize + current_offset - moffset as isize;
            if visible_row < 0 || visible_row >= term_rows {
                continue;
            }
            let row_idx = visible_row as usize;
            if row_idx < rows.len() {
                for char_off in 0..qlen {
                    let col_idx = mc as usize + char_off;
                    if col_idx < rows[row_idx].len() {
                        if mi == active_idx {
                            rows[row_idx][col_idx].is_find_active = true;
                        } else {
                            rows[row_idx][col_idx].is_find_match = true;
                        }
                    }
                }
            }
        }
    }

    rows
}

/// Build the TerminalPanel from engine state (when terminal is open).
pub(crate) fn build_terminal_panel(engine: &Engine) -> Option<TerminalPanel> {
    if !engine.terminal_open {
        return None;
    }

    // Prepare find-highlight data (applies only to the focused/active pane).
    let match_count = engine.terminal_find_matches.len();
    let find_selected_idx = if match_count > 0 {
        engine.terminal_find_selected % match_count
    } else {
        0
    };
    #[allow(clippy::type_complexity)]
    let find_data: Option<(&[(usize, u16, u16)], usize, usize)> =
        if engine.terminal_find_active && match_count > 0 {
            Some((
                &engine.terminal_find_matches,
                engine.terminal_find_query.chars().count(),
                find_selected_idx,
            ))
        } else {
            None
        };

    // ── Split view: two panes side-by-side ────────────────────────────────────
    if engine.terminal_split && engine.terminal_panes.len() >= 2 {
        let left_pane = &engine.terminal_panes[0].session;
        let right_pane = &engine.terminal_panes[1].session;
        let left_cursor_active = engine.terminal_has_focus && engine.terminal_active == 0;
        let right_cursor_active = engine.terminal_has_focus && engine.terminal_active == 1;

        // Find highlights only shown in the focused pane.
        let left_find = if engine.terminal_active == 0 {
            find_data
        } else {
            None
        };
        let right_find = if engine.terminal_active == 1 {
            find_data
        } else {
            None
        };

        let split_left_rows = build_pane_rows(left_pane, left_cursor_active, left_find);
        let rows = build_pane_rows(right_pane, right_cursor_active, right_find);

        // Active pane supplies scroll / scrollback for the scrollbar.
        let active_pane = if engine.terminal_active == 1 {
            right_pane
        } else {
            left_pane
        };

        return Some(TerminalPanel {
            rows,
            content_rows: right_pane.rows(),
            content_cols: right_pane.cols(),
            has_focus: engine.terminal_has_focus,
            scroll_offset: active_pane.scroll_offset(),
            scrollback_rows: active_pane.history_len(),
            tab_count: engine.terminal_panes.len(),
            active_tab: engine.terminal_active,
            find_active: engine.terminal_find_active,
            find_query: engine.terminal_find_query.clone(),
            find_match_count: match_count,
            find_selected_idx,
            split_left_rows: Some(split_left_rows),
            split_left_cols: if engine.terminal_split_left_cols > 0 {
                engine.terminal_split_left_cols
            } else {
                left_pane.cols()
            },
            split_focus: engine.terminal_active as u8,
            maximized: engine.terminal_maximized,
        });
    }

    // ── Single-pane (normal) view ──────────────────────────────────────────────
    let term = engine.active_terminal()?;
    let hist_len = term.history_len();
    let scroll_offset = term.scroll_offset();
    let cursor_active = engine.terminal_has_focus;
    let rows = build_pane_rows(term, cursor_active, find_data);

    Some(TerminalPanel {
        rows,
        content_rows: term.rows(),
        content_cols: term.cols(),
        has_focus: engine.terminal_has_focus,
        scroll_offset,
        scrollback_rows: hist_len,
        tab_count: engine.terminal_panes.len(),
        active_tab: engine.terminal_active,
        find_active: engine.terminal_find_active,
        find_query: engine.terminal_find_query.clone(),
        find_match_count: match_count,
        find_selected_idx,
        split_left_rows: None,
        split_left_cols: 0,
        split_focus: 0,
        maximized: engine.terminal_maximized,
    })
}

/// Build breadcrumb segments for a single editor group.
pub(crate) fn build_breadcrumbs_for_group(
    engine: &Engine,
    group_id: GroupId,
) -> Vec<BreadcrumbSegment> {
    let group = match engine.editor_groups.get(&group_id) {
        Some(g) => g,
        None => return vec![],
    };
    let window_id = group.tabs[group.active_tab].active_window;
    let window = match engine.windows.get(&window_id) {
        Some(w) => w,
        None => return vec![],
    };
    let buf_state = match engine.buffer_manager.get(window.buffer_id) {
        Some(s) => s,
        None => return vec![],
    };

    let mut segments = Vec::new();
    let mut idx = 0usize;

    // Path segments (relative to cwd)
    if let Some(ref file_path) = buf_state.file_path {
        let clean_path = crate::core::paths::strip_unc_prefix(file_path);
        let clean_cwd = crate::core::paths::strip_unc_prefix(&engine.cwd);
        let display = if let Ok(rel) = clean_path.strip_prefix(clean_cwd.as_ref()) {
            rel.to_string_lossy().to_string()
        } else {
            clean_path.to_string_lossy().to_string()
        };
        let parts: Vec<&str> = display.split(std::path::MAIN_SEPARATOR).collect();
        let mut accumulated = engine.cwd.clone();
        for part in &parts {
            accumulated = accumulated.join(part);
            segments.push(BreadcrumbSegment {
                label: part.to_string(),
                is_last: false,
                is_symbol: false,
                index: idx,
                path_prefix: Some(accumulated.clone()),
                symbol_line: None,
            });
            idx += 1;
        }
    }

    // Symbol segments from tree-sitter
    {
        let cursor = &window.view.cursor;
        let text = buf_state.buffer.to_string();
        let scopes = if let Some(ref syn) = buf_state.syntax {
            syn.enclosing_scopes(&text, cursor.line, cursor.col)
        } else {
            Vec::new()
        };
        for scope in scopes {
            segments.push(BreadcrumbSegment {
                label: scope.name,
                is_last: false,
                is_symbol: true,
                index: idx,
                path_prefix: None,
                symbol_line: Some(scope.line),
            });
            idx += 1;
        }
    }

    // Mark the last segment
    if let Some(last) = segments.last_mut() {
        last.is_last = true;
    }

    segments
}
