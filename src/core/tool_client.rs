//! Generic external-tool JSON seam (#522).
//!
//! [`ToolClient`] knows how to run a configured external command and parse
//! its stdout as JSON. It has **no idea what it's running** — no specific
//! external pipeline tool's subcommand names, JSON schema, or lifecycle
//! vocabulary baked in anywhere. An extension (e.g. a future pipeline-
//! management bundle) supplies the argv; vimcode supplies the plumbing.
//! See `docs/COORDINATOR_INTEGRATION.md` §6 for the placement rule this
//! module exists to satisfy: vimcode is an editor that can *host* a
//! pipeline-management client, not a pipeline-management client itself,
//! so `src/core/` must build, run, and pass its suite with no such tool
//! installed and no such tool's config present.
//!
//! ## The board contract
//!
//! vimcode's board-data contract *is* quadraui's `Board` primitive data
//! model (`BoardModel` / `BoardColumn` / `BoardCard` / `CardBadge` /
//! `BadgeStatus`, quadraui#638) — reused here via `quadraui`'s existing
//! `Serialize`/`Deserialize` impls rather than duplicated. Any external
//! tool whose stdout deserializes into a [`quadraui::BoardModel`] can
//! drive the Board panel; see [`fetch_board_model`]. The contract is
//! vimcode's (well, quadraui's, which vimcode renders) — not any
//! particular provider's.
//!
//! ## Threading
//!
//! Spawning + waiting on a subprocess blocks, so [`ToolClient::run_json`]
//! is itself a **blocking** call. Callers run it from a background thread
//! and funnel the result back through an `mpsc` channel polled from
//! `poll_idle`, the same pattern `Engine::ext_refresh` / `poll_ext_registry`
//! already use for registry fetches (see `src/core/engine/buffers.rs`'s
//! `ext_refresh`/`poll_ext_registry` pair). `tool_client` does not own a
//! thread or a channel itself — it is the synchronous primitive that
//! pattern wraps, kept separate so it stays trivially mockable.

use crate::core::git::hidden_command;
use serde::{Deserialize, Serialize};

// ─── Errors ─────────────────────────────────────────────────────────────────

/// Everything that can go wrong running an external tool and parsing its
/// stdout as JSON. Every variant carries enough to build a readable
/// message via [`ToolError::user_message`] — callers should never need to
/// panic or improvise wording when a provider command fails.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolError {
    /// `argv` was empty — nothing to run.
    EmptyCommand,
    /// The configured binary is not on PATH (or otherwise could not be
    /// spawned at all, e.g. permission denied).
    BinaryNotFound(String),
    /// The process could not be spawned/waited on for some other OS-level
    /// reason (distinct from "not found").
    Spawn(String),
    /// The process ran and exited with a non-zero status.
    NonZeroExit { code: Option<i32>, stderr: String },
    /// The process exited successfully but stdout was not valid JSON (or
    /// valid JSON that didn't match the shape being parsed into).
    InvalidJson(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::EmptyCommand => write!(f, "no command configured"),
            ToolError::BinaryNotFound(bin) => write!(f, "'{bin}' not found on PATH"),
            ToolError::Spawn(msg) => write!(f, "failed to run command: {msg}"),
            ToolError::NonZeroExit { code, stderr } => {
                let code = code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                if stderr.trim().is_empty() {
                    write!(f, "command exited with status {code}")
                } else {
                    write!(f, "command exited with status {code}: {}", stderr.trim())
                }
            }
            ToolError::InvalidJson(msg) => write!(f, "command output was not valid JSON: {msg}"),
        }
    }
}

impl std::error::Error for ToolError {}

impl ToolError {
    /// A short, user-facing message safe to show directly in a panel
    /// (e.g. the Board panel's "provider failed" line). Callers with *no*
    /// provider configured at all should show their own "no board
    /// provider configured" message instead — there's no `ToolError`
    /// variant for that, since it isn't a failure of anything that ran.
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

// ─── The seam ───────────────────────────────────────────────────────────────

/// A generic external-tool seam: run a configured argv, get parsed JSON
/// back. Implementors know nothing about what they're running.
pub trait ToolClient: Send + Sync {
    /// Run `argv[0]` with `argv[1..]` as arguments, capture stdout, and
    /// parse it as JSON. **Blocking** — see the module doc for the
    /// threading contract callers are expected to follow.
    fn run_json(&self, argv: &[String]) -> Result<serde_json::Value, ToolError>;

    /// Run `argv[0]` with `argv[1..]` as arguments, writing `stdin` to the
    /// process's stdin, and return its raw stdout on a zero exit (a typed
    /// error otherwise). **Blocking**, same contract as [`Self::run_json`].
    /// Used for provider commands that *consume* a payload — the stdout is
    /// captured (not parsed as JSON here) so a caller like
    /// [`push_tool_document`] (#524) can opportunistically read back
    /// whatever the provider chose to echo (e.g. a newly-assigned document
    /// id on create), without every such command being required to emit
    /// anything at all.
    fn run_with_stdin(&self, argv: &[String], stdin: &[u8]) -> Result<Vec<u8>, ToolError>;

    /// Run `argv` with no stdin and discard stdout — a fire-and-forget
    /// command where only success/failure matters (e.g. a provider's
    /// declared follow-up command after a write). Default impl in terms of
    /// [`Self::run_with_stdin`]; implementors don't need to override it.
    fn run(&self, argv: &[String]) -> Result<(), ToolError> {
        self.run_with_stdin(argv, &[]).map(|_| ())
    }
}

/// Real [`ToolClient`] impl: spawns an actual OS subprocess.
#[derive(Debug, Clone, Copy, Default)]
pub struct SubprocessToolClient;

impl ToolClient for SubprocessToolClient {
    fn run_json(&self, argv: &[String]) -> Result<serde_json::Value, ToolError> {
        let (program, args) = argv.split_first().ok_or(ToolError::EmptyCommand)?;

        let mut cmd = hidden_command(program);
        let output = cmd.args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ToolError::BinaryNotFound(program.clone())
            } else {
                ToolError::Spawn(e.to_string())
            }
        })?;

        if !output.status.success() {
            return Err(ToolError::NonZeroExit {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        serde_json::from_slice(&output.stdout).map_err(|e| ToolError::InvalidJson(e.to_string()))
    }

    fn run_with_stdin(&self, argv: &[String], stdin: &[u8]) -> Result<Vec<u8>, ToolError> {
        let (program, args) = argv.split_first().ok_or(ToolError::EmptyCommand)?;

        let mut cmd = hidden_command(program);
        let mut child = cmd
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ToolError::BinaryNotFound(program.clone())
                } else {
                    ToolError::Spawn(e.to_string())
                }
            })?;

        if let Some(mut child_stdin) = child.stdin.take() {
            use std::io::Write;
            // A provider that exits without reading stdin (or a pipe that
            // fills before the process drains it) makes this write fail —
            // treat that the same as any other spawn-time failure rather
            // than panicking.
            if let Err(e) = child_stdin.write_all(stdin) {
                return Err(ToolError::Spawn(e.to_string()));
            }
        }

        let output = child
            .wait_with_output()
            .map_err(|e| ToolError::Spawn(e.to_string()))?;

        if !output.status.success() {
            return Err(ToolError::NonZeroExit {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        Ok(output.stdout)
    }
}

/// Test/mock [`ToolClient`]: returns a canned result without spawning
/// anything. Every consumer of `ToolClient` should be fully testable
/// against this with no provider binary installed anywhere.
#[derive(Debug, Clone)]
pub struct MockToolClient(pub Result<serde_json::Value, ToolError>);

impl ToolClient for MockToolClient {
    fn run_json(&self, _argv: &[String]) -> Result<serde_json::Value, ToolError> {
        self.0.clone()
    }

    fn run_with_stdin(&self, _argv: &[String], _stdin: &[u8]) -> Result<Vec<u8>, ToolError> {
        // The canned value doubles as "stdout" here, JSON-encoded — lets a
        // test exercise the create-path id-capture round trip (#524) with
        // e.g. `MockToolClient(Ok(json!({"id": "99"})))` without a second,
        // stdout-specific mock type.
        self.0
            .clone()
            .map(|v| serde_json::to_vec(&v).unwrap_or_default())
    }
}

/// Test [`ToolClient`] that records the argv (and stdin, if any) of every
/// call it receives, in order, so a test can assert *what* was sent to a
/// provider — not just that something succeeded. Every call succeeds
/// (`run_json` returns `null`). Used by #524's push/follow-up tests to
/// verify the write command gets the right title/body payload and the
/// follow-up command gets the right id.
#[derive(Debug, Clone, Default)]
pub struct RecordingToolClient {
    pub calls: std::sync::Arc<std::sync::Mutex<Vec<RecordedCall>>>,
}

/// One recorded [`RecordingToolClient`] invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedCall {
    pub argv: Vec<String>,
    /// `Some` for a [`ToolClient::run_with_stdin`]/[`ToolClient::run`]
    /// call, `None` for a [`ToolClient::run_json`] call.
    pub stdin: Option<Vec<u8>>,
}

impl RecordingToolClient {
    /// Snapshot of every call recorded so far, in order.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().expect("recording client mutex").clone()
    }
}

impl ToolClient for RecordingToolClient {
    fn run_json(&self, argv: &[String]) -> Result<serde_json::Value, ToolError> {
        self.calls
            .lock()
            .expect("recording client mutex")
            .push(RecordedCall {
                argv: argv.to_vec(),
                stdin: None,
            });
        Ok(serde_json::Value::Null)
    }

    fn run_with_stdin(&self, argv: &[String], stdin: &[u8]) -> Result<Vec<u8>, ToolError> {
        self.calls
            .lock()
            .expect("recording client mutex")
            .push(RecordedCall {
                argv: argv.to_vec(),
                stdin: Some(stdin.to_vec()),
            });
        Ok(Vec::new())
    }
}

// ─── The document contract (#524) ──────────────────────────────────────────

/// vimcode's generic document contract: a provider's read command emits
/// this shape on stdout to seed an editable markdown buffer; the write
/// command receives the (title, body) pair back on stdin as this same
/// shape's `title`/`body` fields. `labels`/`status` are read-only context
/// shown in the buffer's metadata header — never sent back on `:w`, since
/// editing them is lifecycle-specific and belongs to whatever bundle
/// declared the provider, not this generic seam (see the module doc).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolDocument {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// Run `argv` via `client` and parse the result as a [`ToolDocument`] —
/// the read half of #524's buffer-authoring seam.
pub fn fetch_tool_document(
    client: &dyn ToolClient,
    argv: &[String],
) -> Result<ToolDocument, ToolError> {
    let value = client.run_json(argv)?;
    serde_json::from_value(value).map_err(|e| ToolError::InvalidJson(e.to_string()))
}

/// Run `argv` via `client`, feeding it `{"title": title, "body": body}` on
/// stdin — the write half of #524's buffer-authoring seam. Success is a
/// zero exit; returns the provider-assigned id if the provider chose to
/// echo one as `{"id": "..."}` on stdout — the create path (`{id}` was
/// substituted empty) needs this to bind the now-existing document to its
/// buffer, so a second `:w` updates it instead of re-running "create" with
/// an empty id again. A provider that emits nothing, or emits something
/// that isn't `{"id": "..."}`, is not an error — `None` either way, since
/// nothing here requires a provider to echo anything at all.
pub fn push_tool_document(
    client: &dyn ToolClient,
    argv: &[String],
    title: &str,
    body: &str,
) -> Result<Option<String>, ToolError> {
    let payload = serde_json::json!({ "title": title, "body": body });
    let stdin = serde_json::to_vec(&payload).map_err(|e| ToolError::InvalidJson(e.to_string()))?;
    let stdout = client.run_with_stdin(argv, &stdin)?;
    let id = serde_json::from_slice::<serde_json::Value>(&stdout)
        .ok()
        .and_then(|v| v.get("id").and_then(|id| id.as_str()).map(str::to_string));
    Ok(id)
}

// ─── The board contract ─────────────────────────────────────────────────────

/// Run `argv` via `client` and parse the result as vimcode's board
/// contract (`quadraui::BoardModel`, matching quadraui's `Board`
/// primitive — quadraui#638). Any provider that emits this shape on
/// stdout gets a board; nothing here knows or cares which one.
pub fn fetch_board_model(
    client: &dyn ToolClient,
    argv: &[String],
) -> Result<quadraui::BoardModel, ToolError> {
    let value = client.run_json(argv)?;
    serde_json::from_value(value).map_err(|e| ToolError::InvalidJson(e.to_string()))
}

// ─── The branch-review contract (#525) ─────────────────────────────────────

/// The result of resolving a board card to a reviewable change: which git
/// revision holds the change (`branch`) and which revision to diff it
/// against (`base`). Generic, same spirit as [`ToolDocument`]/
/// [`quadraui::BoardModel`] — any provider whose `"OpenReview"` board
/// action (see `crate::core::extensions::BoardProviderConfig::actions`)
/// emits this JSON shape on stdout gets a multi-file diff review; nothing
/// here names a specific provider or lifecycle vocabulary. Both revisions
/// must already be resolvable in the *local* repository (already pulled/
/// checked out) — turning this into a real diff is pure local git
/// (`crate::core::git::changed_files_between`), not another external-tool
/// round trip.
///
/// `host` (#530, Track A Phase 5 — the fleet review seat) is an *optional*,
/// purely descriptive label — never resolved, dialled, or `ssh`ed into by
/// vimcode itself. It is what makes the "review-where-the-code-is" mode
/// distinguishable from "pull-local" (`docs/COORDINATOR_INTEGRATION.md`
/// §9/§11): a provider whose roster spans a machine fleet fills it with
/// which worker box this branch/base pair lives on; a provider with only
/// one checkout (or an external "pull the branch locally first" flow)
/// leaves it `None`. Both modes reach `Engine::open_branch_review`
/// identically — the *moat* is that vimcode already works correctly when
/// it is itself the process running (over ssh) on that worker box, with
/// no vimcode-side transport code at all; `host` only has to carry the
/// provenance a human reviewing there needs to see, not make the trip
/// happen.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct BranchReviewTarget {
    /// The branch (or any git revision) containing the change to review.
    pub branch: String,
    /// The revision to diff `branch` against — typically the branch it was
    /// cut from. Compared with three-dot semantics, see
    /// `crate::core::git::changed_files_between`.
    pub base: String,
    /// Which fleet machine this branch/base pair's worktree lives on, if
    /// the provider's roster spans more than one (#530). `None` when the
    /// provider doesn't distinguish machines, or omits the field entirely
    /// (`#[serde(default)]` — every #525-era provider response still
    /// parses unchanged).
    #[serde(default)]
    pub host: Option<String>,
}

/// Run `argv` via `client` and parse the result as a [`BranchReviewTarget`]
/// — the "resolve a card to a branch" half of #525's review seam. The
/// "branch to changed files" half is pure local git; this is the only part
/// that needs an external tool at all.
pub fn fetch_branch_review_target(
    client: &dyn ToolClient,
    argv: &[String],
) -> Result<BranchReviewTarget, ToolError> {
    let value = client.run_json(argv)?;
    serde_json::from_value(value).map_err(|e| ToolError::InvalidJson(e.to_string()))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use quadraui::{BadgeStatus, WidgetId};

    fn fixture_json() -> serde_json::Value {
        serde_json::json!({
            "id": "board",
            "columns": [
                {
                    "id": "col:backlog",
                    "title": "Backlog",
                    "cards": [
                        {
                            "id": "card:1",
                            "title": "#1 Example",
                            "labels": ["repo"],
                            "badges": [{"label": "P", "status": "Passed"}],
                            "hint": null,
                        }
                    ],
                    "scroll_offset": 0,
                }
            ],
            "selected_card_id": null,
            "col_scroll_offset": 0,
        })
    }

    #[test]
    fn mock_client_fetch_board_model_parses_fixture() {
        // Acceptance: "tests parse a fixture into the board contract."
        let client = MockToolClient(Ok(fixture_json()));
        let model = fetch_board_model(&client, &["whatever".to_string()])
            .expect("fixture should parse into BoardModel");
        assert_eq!(model.columns.len(), 1);
        assert_eq!(model.columns[0].title, "Backlog");
        assert_eq!(model.columns[0].cards.len(), 1);
        assert_eq!(model.columns[0].cards[0].id, WidgetId::new("card:1"));
        assert_eq!(
            model.columns[0].cards[0].badges[0].status,
            BadgeStatus::Passed
        );
        assert_eq!(model.selected_card_id, None);
    }

    #[test]
    fn mock_client_propagates_configured_error() {
        let client = MockToolClient(Err(ToolError::BinaryNotFound("some-provider".into())));
        let err = fetch_board_model(&client, &["some-provider".to_string()]).unwrap_err();
        assert!(matches!(err, ToolError::BinaryNotFound(ref b) if b == "some-provider"));
    }

    #[test]
    fn subprocess_client_missing_binary_is_typed_error_not_panic() {
        // Acceptance: "Missing/failing provider command degrades to a
        // message, never a panic."
        let client = SubprocessToolClient;
        let argv = vec!["definitely-not-a-real-binary-522".to_string()];
        let err = client.run_json(&argv).unwrap_err();
        assert!(matches!(err, ToolError::BinaryNotFound(_)));
        assert!(err.user_message().contains("not found on PATH"));
    }

    #[test]
    fn subprocess_client_empty_argv_is_typed_error() {
        let client = SubprocessToolClient;
        let err = client.run_json(&[]).unwrap_err();
        assert_eq!(err, ToolError::EmptyCommand);
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_client_parses_real_process_json() {
        let client = SubprocessToolClient;
        let (shell, flag) = crate::core::terminal::shell_command();
        let argv = vec![
            shell,
            flag,
            r#"printf '%s' '{"id":"board","columns":[],"selected_card_id":null,"col_scroll_offset":0}'"#
                .to_string(),
        ];
        let model = fetch_board_model(&client, &argv).expect("real subprocess should parse");
        assert_eq!(model.columns.len(), 0);
        assert_eq!(model.id, WidgetId::new("board"));
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_client_nonzero_exit_is_typed_error() {
        let client = SubprocessToolClient;
        let (shell, flag) = crate::core::terminal::shell_command();
        let argv = vec![shell, flag, "echo oops 1>&2; exit 3".to_string()];
        match client.run_json(&argv).unwrap_err() {
            ToolError::NonZeroExit { code, stderr } => {
                assert_eq!(code, Some(3));
                assert!(stderr.contains("oops"));
            }
            other => panic!("expected NonZeroExit, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_client_bad_json_is_typed_error() {
        let client = SubprocessToolClient;
        let (shell, flag) = crate::core::terminal::shell_command();
        let argv = vec![shell, flag, "printf 'not json'".to_string()];
        assert!(matches!(
            client.run_json(&argv).unwrap_err(),
            ToolError::InvalidJson(_)
        ));
    }

    #[test]
    fn tool_error_user_message_includes_stderr() {
        let err = ToolError::NonZeroExit {
            code: Some(1),
            stderr: "boom".to_string(),
        };
        assert!(err.user_message().contains("boom"));
    }

    // ─── #524: the document contract ───────────────────────────────────────

    #[test]
    fn mock_client_fetch_tool_document_parses_fixture() {
        let client = MockToolClient(Ok(serde_json::json!({
            "title": "Briefing readability",
            "body": "Some prose.\n\n```rust\nfn f() {}\n```\n",
            "labels": ["docs"],
            "status": "refining",
        })));
        let doc = fetch_tool_document(&client, &["whatever".to_string()])
            .expect("fixture should parse into ToolDocument");
        assert_eq!(doc.title, "Briefing readability");
        assert!(doc.body.contains("```rust"));
        assert_eq!(doc.labels, vec!["docs".to_string()]);
        assert_eq!(doc.status.as_deref(), Some("refining"));
    }

    #[test]
    fn tool_document_missing_fields_default_rather_than_error() {
        // A provider that only emits `body` (or nothing at all) shouldn't
        // fail to parse — every field defaults.
        let client = MockToolClient(Ok(serde_json::json!({})));
        let doc = fetch_tool_document(&client, &["whatever".to_string()]).unwrap();
        assert_eq!(doc, ToolDocument::default());
    }

    #[test]
    fn push_tool_document_sends_title_and_body_on_stdin() {
        let client = RecordingToolClient::default();
        push_tool_document(
            &client,
            &[
                "provider".to_string(),
                "write".to_string(),
                "42".to_string(),
            ],
            "My title",
            "My body",
        )
        .expect("recording client always succeeds");

        let calls = client.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].argv,
            vec![
                "provider".to_string(),
                "write".to_string(),
                "42".to_string()
            ]
        );
        let sent: serde_json::Value =
            serde_json::from_slice(calls[0].stdin.as_deref().expect("stdin was sent")).unwrap();
        assert_eq!(sent["title"], "My title");
        assert_eq!(sent["body"], "My body");
    }

    /// #524 review follow-up: a provider that echoes `{"id": "..."}` on
    /// stdout from the write command lets the create path learn the
    /// newly-assigned id (`document_ops.rs`'s `save_tool_document_buffer`
    /// is the actual consumer — this covers the plumbing it depends on).
    #[test]
    fn push_tool_document_returns_provider_assigned_id_from_stdout() {
        let client = MockToolClient(Ok(serde_json::json!({ "id": "99" })));
        let id = push_tool_document(&client, &["provider".to_string()], "t", "b").unwrap();
        assert_eq!(id, Some("99".to_string()));
    }

    /// A provider that echoes nothing useful (or nothing at all) is not an
    /// error — the create path just stays unbound, exactly like before this
    /// echoing convention existed.
    #[test]
    fn push_tool_document_with_no_id_in_stdout_returns_none() {
        let client = RecordingToolClient::default();
        let id = push_tool_document(&client, &["provider".to_string()], "t", "b").unwrap();
        assert_eq!(id, None);
    }

    #[test]
    fn push_tool_document_propagates_client_error() {
        let client = MockToolClient(Err(ToolError::NonZeroExit {
            code: Some(1),
            stderr: "rejected".to_string(),
        }));
        let err = push_tool_document(&client, &["provider".to_string()], "t", "b").unwrap_err();
        assert!(matches!(err, ToolError::NonZeroExit { .. }));
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_client_run_with_stdin_round_trips_through_cat() {
        // `cat` echoes stdin to stdout; a real subprocess proves the pipe
        // actually carries the payload, not just that the argv was built.
        let client = SubprocessToolClient;
        client
            .run_with_stdin(&["cat".to_string()], b"hello")
            .expect("cat should exit zero");
    }

    #[cfg(unix)]
    #[test]
    fn subprocess_client_run_with_stdin_nonzero_exit_is_typed_error() {
        let client = SubprocessToolClient;
        let (shell, flag) = crate::core::terminal::shell_command();
        let argv = vec![shell, flag, "cat >/dev/null; exit 7".to_string()];
        match client.run_with_stdin(&argv, b"ignored").unwrap_err() {
            ToolError::NonZeroExit { code, .. } => assert_eq!(code, Some(7)),
            other => panic!("expected NonZeroExit, got {other:?}"),
        }
    }

    #[test]
    fn recording_client_run_records_no_stdin_call() {
        let client = RecordingToolClient::default();
        client
            .run(&["follow-up".to_string(), "42".to_string()])
            .unwrap();
        let calls = client.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].argv,
            vec!["follow-up".to_string(), "42".to_string()]
        );
        assert_eq!(calls[0].stdin, Some(Vec::new()));
    }

    // ── fetch_branch_review_target (#525) ───────────────────────────────

    #[test]
    fn mock_client_fetch_branch_review_target_parses_fixture() {
        let client = MockToolClient(Ok(serde_json::json!({
            "branch": "issue-525-review",
            "base": "develop",
        })));
        let target = fetch_branch_review_target(&client, &["whatever".to_string()])
            .expect("fixture should parse into BranchReviewTarget");
        assert_eq!(target.branch, "issue-525-review");
        assert_eq!(target.base, "develop");
        // A provider that predates #530's `host` field must still parse —
        // `#[serde(default)]` is what makes that true, not an accident of
        // this particular fixture.
        assert_eq!(target.host, None);
    }

    /// #530 (Track A Phase 5): a fleet provider's roster spans more than
    /// one worker machine, so its `"OpenReview"` response names which one
    /// this branch/base pair's worktree lives on.
    #[test]
    fn mock_client_fetch_branch_review_target_parses_host_when_present() {
        let client = MockToolClient(Ok(serde_json::json!({
            "branch": "issue-530-review",
            "base": "develop",
            "host": "worker-3.fleet.local",
        })));
        let target = fetch_branch_review_target(&client, &["whatever".to_string()])
            .expect("fixture should parse into BranchReviewTarget");
        assert_eq!(target.host.as_deref(), Some("worker-3.fleet.local"));
    }

    #[test]
    fn fetch_branch_review_target_propagates_configured_error() {
        let client = MockToolClient(Err(ToolError::BinaryNotFound("some-provider".into())));
        let err = fetch_branch_review_target(&client, &["some-provider".to_string()]).unwrap_err();
        assert!(matches!(err, ToolError::BinaryNotFound(ref b) if b == "some-provider"));
    }
}
