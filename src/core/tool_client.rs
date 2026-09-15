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

use std::process::Command;

use crate::core::git::hidden_command;

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
}

/// Real [`ToolClient`] impl: spawns an actual OS subprocess.
#[derive(Debug, Clone, Copy, Default)]
pub struct SubprocessToolClient;

impl ToolClient for SubprocessToolClient {
    fn run_json(&self, argv: &[String]) -> Result<serde_json::Value, ToolError> {
        let (program, args) = argv.split_first().ok_or(ToolError::EmptyCommand)?;

        let mut cmd: Command = hidden_command(program);
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
        let argv = vec![
            "sh".to_string(),
            "-c".to_string(),
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
        let argv = vec![
            "sh".to_string(),
            "-c".to_string(),
            "echo oops 1>&2; exit 3".to_string(),
        ];
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
        let argv = vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf 'not json'".to_string(),
        ];
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
}
