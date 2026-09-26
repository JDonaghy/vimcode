//! Source-agnostic agent task-plan model (#529, Track A Phase 4 —
//! observability: "plan preview").
//!
//! A structured plan is an ordered checklist of steps with status. Two
//! producers exist in this tree today — a live ACP session's
//! `session/update` `plan` variant (`core::acp`), parsed inside that
//! module's own envelope check — and #529's future Board/remote-worker
//! plan preview, fed through a completely different transport
//! (`ToolClient`, not the ACP wire). Both need to render identically, so
//! the *model* (this module) and its rendering (`plan_to_checklist_text`,
//! consumed by `render::populate_ai_chat_controller`) live here, with
//! nothing ACP-specific anywhere in it — only the wire envelope
//! (`{"sessionUpdate": "plan", ...}`) is ACP's, and that check stays in
//! `core::acp::parse_plan_update`, layered on top of
//! [`parse_plan_entries`] rather than duplicating its entry-parsing logic.
//!
//! `core::acp` re-exports [`PlanEntry`]/[`PlanEntryStatus`] as
//! `AcpPlanEntry`/`AcpPlanEntryStatus` so existing ACP call sites
//! (`AcpSession::plan` (via `Engine::acp()`), `render::populate_ai_chat_controller`) are
//! unaffected by this module's existence — this is a relocation of the
//! model to a source-agnostic home, not a new parallel type.

/// Status of one plan entry. Unknown/missing values default to
/// [`Self::Pending`] — never [`Self::Completed`], since defaulting a step
/// toward "already done" on malformed/absent input would hide unfinished
/// work from the checklist rather than merely under-describing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanEntryStatus {
    Pending,
    InProgress,
    Completed,
}

/// One entry in a task-plan breakdown. Deliberately carries nothing beyond
/// what any producer's plan would supply — no field here ties it to ACP,
/// coordinator, or any other specific feeder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    pub content: String,
    pub status: PlanEntryStatus,
}

/// Parse the shared plan-entry shape — `{"entries": [{"content", "status"}]}`
/// — out of `value`. Callers that need an envelope check first (ACP's
/// `sessionUpdate == "plan"`, or a future Board plan-preview command's own
/// marker) do that themselves and pass the same object through; this
/// function only ever looks at `entries`, so it has no opinion on how a
/// plan was announced, only on what one looks like.
///
/// Returns `None` when `entries` is missing or not an array — malformed,
/// not the same as a *valid* empty plan (`Some(vec![])`), which clears
/// whatever was rendered before.
///
/// An entry missing `content` is dropped rather than rendered as a blank
/// checklist row; a missing/unrecognized `status` defaults to `Pending`.
pub fn parse_plan_entries(value: &serde_json::Value) -> Option<Vec<PlanEntry>> {
    let entries = value.get("entries").and_then(|v| v.as_array())?;
    Some(
        entries
            .iter()
            .filter_map(|e| {
                let content = e.get("content")?.as_str()?.to_string();
                let status = match e.get("status").and_then(|v| v.as_str()) {
                    Some("in_progress") => PlanEntryStatus::InProgress,
                    Some("completed") => PlanEntryStatus::Completed,
                    _ => PlanEntryStatus::Pending,
                };
                Some(PlanEntry { content, status })
            })
            .collect(),
    )
}

/// Render a plan as a plain-text checklist for the AI panel transcript
/// (`render::populate_ai_chat_controller`). Unicode checkbox glyphs
/// (`\u{2610}`/`\u{2611}`) rather than Markdown `- [ ]`/`- [x]` syntax,
/// since the transcript's plain `StyledText` path (not
/// `ChatController::push_turn_markdown`'s list-aware renderer) is what
/// every other turn in that panel already uses.
pub fn plan_to_checklist_text(entries: &[PlanEntry]) -> String {
    let mut out = String::from("Plan:\n");
    for e in entries {
        let glyph = match e.status {
            PlanEntryStatus::Completed => "\u{2611}",
            _ => "\u{2610}",
        };
        let suffix = match e.status {
            PlanEntryStatus::InProgress => " (in progress)",
            _ => "",
        };
        out.push_str(&format!("{glyph} {}{suffix}\n", e.content));
    }
    out.pop(); // drop the trailing newline
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #529's acceptance bar: the plan model must be usable with **no**
    /// ACP envelope at all — a plain `{"entries": [...]}` object, the
    /// shape a future Board/remote-worker plan-preview command would emit
    /// on its own stdout via `ToolClient::run_json`, with no
    /// `"sessionUpdate"` field anywhere. If this function secretly needed
    /// ACP's wrapper it would return `None` here.
    #[test]
    fn parse_plan_entries_has_no_acp_envelope_requirement() {
        let board_shaped = serde_json::json!({
            "entries": [
                {"content": "Refine the issue", "status": "completed"},
                {"content": "Dispatch to a worker", "status": "in_progress"},
                {"content": "Review the diff", "status": "pending"},
            ],
        });
        let plan = parse_plan_entries(&board_shaped).expect("no envelope required");
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].status, PlanEntryStatus::Completed);
        assert_eq!(plan[1].status, PlanEntryStatus::InProgress);
        assert_eq!(plan[2].status, PlanEntryStatus::Pending);
    }

    #[test]
    fn parse_plan_entries_missing_entries_is_none() {
        assert_eq!(parse_plan_entries(&serde_json::json!({})), None);
    }

    #[test]
    fn parse_plan_entries_defaults_missing_status_to_pending() {
        let value = serde_json::json!({"entries": [{"content": "step"}]});
        let plan = parse_plan_entries(&value).expect("should parse");
        assert_eq!(plan[0].status, PlanEntryStatus::Pending);
    }

    #[test]
    fn plan_to_checklist_text_renders_regardless_of_producer() {
        // Built directly, with no parser at all — the point being that
        // rendering has no dependency on *how* the entries arrived.
        let entries = vec![
            PlanEntry {
                content: "done step".to_string(),
                status: PlanEntryStatus::Completed,
            },
            PlanEntry {
                content: "active step".to_string(),
                status: PlanEntryStatus::InProgress,
            },
        ];
        let text = plan_to_checklist_text(&entries);
        assert!(text.contains("\u{2611} done step"));
        assert!(text.contains("\u{2610} active step (in progress)"));
    }
}
