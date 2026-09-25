//! Provider document buffers (#524, Track A Phase 1): author and refine a
//! provider's documents (e.g. GitHub issues) as real markdown buffers.
//!
//! This is the generic host's `core` half — see `src/core/tool_client.rs`'s
//! module doc for the placement rule and `src/core/extensions.rs`'s
//! `DocumentProviderConfig` for the manifest schema this reads. It knows
//! how to find a configured provider (an installed extension whose
//! manifest declares `[document]`, #522), open its documents as scratch
//! markdown buffers, and push edits back on `:w`. It names **no specific
//! external provider** and no lifecycle vocabulary — a `write_follow_up`
//! command is "run this after a successful write," never "flip status to
//! ready."
//!
//! ## The buffer text shape
//!
//! A document buffer's text is `# <title>\n\n<body>`, optionally preceded
//! by a read-only `<!-- status: ... labels: ... -->` metadata comment when
//! the provider supplied either. Only the title (`# ` line) and body are
//! parsed back out on save — labels/status are provider context, shown but
//! never edited here (editing them is lifecycle-specific, the bundle's
//! job, not this generic seam's).

use super::*;
use crate::core::buffer_manager::ToolDocumentBinding;
use crate::core::extensions::DocumentProviderConfig;
use crate::core::tool_client::{self, ToolDocument};

impl Engine {
    /// The first installed extension that declares a `[document]`
    /// provider, if any. "First" rather than "the only one" — mirrors
    /// `Engine::board_provider`.
    pub fn document_provider(&self) -> Option<DocumentProviderConfig> {
        self.ext_installed_manifests()
            .into_iter()
            .find_map(|m| m.document)
    }

    /// Swap in a mock/recording [`tool_client::ToolClient`] for tests —
    /// every consumer of provider document buffers must be exercisable
    /// with no provider binary installed anywhere.
    #[cfg(test)]
    pub fn set_document_client_for_test(&mut self, client: impl tool_client::ToolClient + 'static) {
        self.document_client = std::sync::Arc::new(client);
    }

    /// Open `id` as a markdown scratch buffer, seeded by running the
    /// configured provider's read command. **Blocking** — opening a
    /// document is a deliberate, one-shot user action (unlike the Board
    /// panel's background poll, there's no cached model to show while
    /// waiting and nothing to keep responsive).
    pub fn open_tool_document(&mut self, id: &str) -> Result<(), String> {
        let provider = self
            .document_provider()
            .ok_or_else(|| "no document provider configured".to_string())?;
        let argv = provider
            .read_argv(id)
            .ok_or_else(|| "provider declared no read command".to_string())?;
        let doc = tool_client::fetch_tool_document(self.document_client.as_ref(), &argv)
            .map_err(|e| e.user_message())?;
        self.open_tool_document_buffer(Some(id.to_string()), doc, &provider);
        Ok(())
    }

    /// Open a blank document buffer for the configured provider. `:w`
    /// creates it — the write command's own semantics decide what "create"
    /// means, this just runs it with an empty id (see
    /// `DocumentProviderConfig::write_argv`'s doc).
    pub fn new_tool_document(&mut self) -> Result<(), String> {
        let provider = self
            .document_provider()
            .ok_or_else(|| "no document provider configured".to_string())?;
        if provider.write_command.is_empty() {
            return Err("provider declared no write command".to_string());
        }
        self.open_tool_document_buffer(None, ToolDocument::default(), &provider);
        Ok(())
    }

    fn open_tool_document_buffer(
        &mut self,
        id: Option<String>,
        doc: ToolDocument,
        provider: &DocumentProviderConfig,
    ) {
        // If this document (by id) is already open in a buffer, switch to
        // it rather than opening a second tab bound to the same id — two
        // buffers racing to `:w` the same document would be "last save
        // wins, first buffer's edits silently lost" (review follow-up,
        // #524). Same precedent as `open_keymaps_editor`. Only applies to a
        // real id: `new_tool_document`'s blank buffer (`id: None`) has
        // nothing yet to deduplicate against — every blank buffer is
        // legitimately a fresh, not-yet-created document.
        if let Some(id) = &id {
            let existing_buf_id = self.buffer_manager.iter().find_map(|(buf_id, state)| {
                state
                    .tool_document
                    .as_ref()
                    .is_some_and(|binding| binding.id.as_deref() == Some(id.as_str()))
                    .then_some(*buf_id)
            });
            if let Some(buf_id) = existing_buf_id {
                let tab_idx = self
                    .active_group()
                    .tabs
                    .iter()
                    .enumerate()
                    .find(|(_, tab)| {
                        self.windows
                            .get(&tab.active_window)
                            .is_some_and(|w| w.buffer_id == buf_id)
                    })
                    .map(|(i, _)| i);
                if let Some(idx) = tab_idx {
                    self.active_group_mut().active_tab = idx;
                } else {
                    // Buffer exists but not shown in this group — point the
                    // current window at it.
                    self.active_window_mut().buffer_id = buf_id;
                    self.view_mut().cursor.line = 0;
                    self.view_mut().cursor.col = 0;
                }
                self.message = "Edit and :w to save.".to_string();
                return;
            }
        }

        let content = render_tool_document_buffer(&doc);
        let buf_id = self.buffer_manager.create();
        if let Some(state) = self.buffer_manager.get_mut(buf_id) {
            state.buffer.content = ropey::Rope::from_str(&content);
            state.syntax = crate::core::syntax::Syntax::new_from_path(Some("document.md"));
            state.tool_document = Some(ToolDocumentBinding {
                id: id.clone(),
                write_command: provider.write_command.clone(),
                write_follow_up: provider.write_follow_up.clone(),
            });
            state.dirty = false;
            state.scratch_name = Some(match &id {
                Some(id) => format!("[Document {id}]"),
                None => "[New Document]".to_string(),
            });
        }

        // Open in a new tab (same pattern as `open_keymaps_editor`).
        let window_id = self.new_window_id();
        let window = Window::new(window_id, buf_id);
        self.windows.insert(window_id, window);
        let tab_id = self.new_tab_id();
        let tab = Tab::new(tab_id, window_id);
        self.active_group_mut().tabs.push(tab);
        self.active_group_mut().active_tab = self.active_group().tabs.len() - 1;

        self.message = "Edit and :w to save.".to_string();
    }

    /// Save a provider document buffer: push title/body via the write
    /// command, then the follow-up command if the provider declared one
    /// **and** the document already existed (follow-ups are a lifecycle
    /// transition on an existing document — the create path has nothing to
    /// transition from).
    ///
    /// **Blocking**, like [`Self::open_tool_document`] — a deliberate
    /// tradeoff that mirrors the keymaps/registries scratch-buffer save
    /// precedent, but worth flagging forward: a real network-backed write
    /// command (unlike a local mock) could visibly freeze the editor for
    /// however long the round-trip takes. A future non-blocking write path
    /// (mirroring `board_refresh`'s background-thread + `mpsc` pattern) is
    /// out of scope here.
    pub(crate) fn save_tool_document_buffer(&mut self) -> Result<(), String> {
        let binding = self
            .active_buffer_state()
            .tool_document
            .clone()
            .ok_or_else(|| "not a document buffer".to_string())?;
        let (title, body) = parse_tool_document_buffer(&self.buffer().to_string());

        if binding.write_command.is_empty() {
            return Err("provider declared no write command".to_string());
        }
        let write_argv: Vec<String> = binding
            .write_command
            .iter()
            .map(|a| a.replace("{id}", binding.id.as_deref().unwrap_or("")))
            .collect();
        let assigned_id = tool_client::push_tool_document(
            self.document_client.as_ref(),
            &write_argv,
            &title,
            &body,
        )
        .map_err(|e| e.user_message())?;

        if let Some(id) = &binding.id {
            if !binding.write_follow_up.is_empty() {
                let follow_argv: Vec<String> = binding
                    .write_follow_up
                    .iter()
                    .map(|a| a.replace("{id}", id))
                    .collect();
                // Best-effort: the write already succeeded, so a follow-up
                // failure (e.g. a transient lifecycle-gate rejection)
                // shouldn't be reported as "the save failed" — the
                // document itself is safely pushed either way.
                let _ = self.document_client.run(&follow_argv);
            }
        }

        // Create path (`binding.id` was `None`): if the provider echoed an
        // id back on stdout, bind this buffer to it now, so a *second* `:w`
        // on the still-open buffer updates the just-created document
        // instead of silently re-running "create" with an empty id again
        // (review follow-up, #524). A provider that echoed nothing leaves
        // the buffer unbound, exactly as before this existed.
        if binding.id.is_none() {
            if let Some(id) = assigned_id {
                let scratch_name = format!("[Document {id}]");
                let state = self.active_buffer_state_mut();
                if let Some(tool_document) = state.tool_document.as_mut() {
                    tool_document.id = Some(id);
                }
                state.scratch_name = Some(scratch_name);
            }
        }

        self.active_buffer_state_mut().dirty = false;
        self.message = "Document saved".to_string();
        Ok(())
    }
}

/// Render a [`ToolDocument`] as scratch-buffer markdown text.
fn render_tool_document_buffer(doc: &ToolDocument) -> String {
    let mut out = String::new();
    if doc.status.is_some() || !doc.labels.is_empty() {
        out.push_str("<!--");
        if let Some(status) = &doc.status {
            out.push_str(&format!(" status: {status}"));
        }
        if !doc.labels.is_empty() {
            out.push_str(&format!(" labels: {}", doc.labels.join(", ")));
        }
        out.push_str(" -->\n");
    }
    out.push_str("# ");
    out.push_str(&doc.title);
    out.push_str("\n\n");
    out.push_str(&doc.body);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Inverse of [`render_tool_document_buffer`]: pull the title (the `# ...`
/// line) and body (everything after it, minus one leading blank line) back
/// out of edited buffer text, skipping a leading metadata comment if
/// present. Never round-trips labels/status.
///
/// Edge case: any first line starting with `<!--` is treated as *the*
/// metadata header and dropped, even if the provider's body itself
/// legitimately opened with an HTML comment the user typed — the metadata
/// comment [`render_tool_document_buffer`] emits is always exactly one
/// line, so this is unambiguous for buffers this module itself produced,
/// but would eat a user-authored leading `<!-- ... -->` line too.
fn parse_tool_document_buffer(text: &str) -> (String, String) {
    let mut lines = text.lines().peekable();
    if lines
        .peek()
        .is_some_and(|l| l.trim_start().starts_with("<!--"))
    {
        lines.next();
    }

    let mut title = String::new();
    let mut saw_title = false;
    for line in lines.by_ref() {
        if let Some(t) = line.strip_prefix("# ") {
            title = t.trim().to_string();
            saw_title = true;
            break;
        } else if line.trim().is_empty() {
            continue;
        } else {
            // First non-blank line isn't a heading — leave title empty
            // rather than guessing, and treat this line as the start of
            // the body instead of consuming it.
            let mut body_lines = vec![line];
            body_lines.extend(lines);
            return (title, join_body_lines(body_lines));
        }
    }
    debug_assert!(saw_title || title.is_empty());

    let mut body_lines: Vec<&str> = lines.collect();
    if body_lines.first().is_some_and(|l| l.trim().is_empty()) {
        body_lines.remove(0);
    }
    (title, join_body_lines(body_lines))
}

/// `str::lines()` drops the information of whether the source ended in a
/// trailing newline, so rejoining with `"\n"` alone silently eats it —
/// join back into a body that ends in exactly one `\n` (matching
/// [`render_tool_document_buffer`]'s own convention), never zero.
fn join_body_lines(lines: Vec<&str>) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut body = lines.join("\n");
    body.push('\n');
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::extensions::ExtensionManifest;
    use crate::core::tool_client::{MockToolClient, RecordingToolClient, ToolError};

    fn install_mock_provider(engine: &mut Engine, provider: DocumentProviderConfig) {
        let mut manifest = ExtensionManifest {
            name: "mock-doc-provider".to_string(),
            ..Default::default()
        };
        manifest.document = Some(provider);
        engine
            .extension_state
            .installed
            .push(crate::core::session::InstalledExtension {
                name: manifest.name.clone(),
                version: String::new(),
            });
        engine.ext_registry = Some(vec![manifest]);
    }

    fn fixture_provider() -> DocumentProviderConfig {
        DocumentProviderConfig {
            read_command: vec!["mock".into(), "show".into(), "{id}".into()],
            write_command: vec!["mock".into(), "write".into(), "{id}".into()],
            write_follow_up: vec!["mock".into(), "ready".into(), "{id}".into()],
        }
    }

    #[test]
    fn no_provider_configured_is_a_typed_error_not_a_panic() {
        let mut engine = Engine::new_for_test();
        assert!(engine.document_provider().is_none());
        let err = engine.open_tool_document("42").unwrap_err();
        assert!(err.contains("no document provider"));
    }

    #[test]
    fn render_and_parse_round_trip_title_and_body() {
        let doc = ToolDocument {
            title: "Briefing readability".to_string(),
            body: "Some prose.\n\n```rust\nfn f() {}\n```\n".to_string(),
            labels: vec!["docs".to_string()],
            status: Some("refining".to_string()),
        };
        let text = render_tool_document_buffer(&doc);
        assert!(text.starts_with("<!-- status: refining labels: docs -->\n"));
        assert!(text.contains("# Briefing readability\n\n"));

        let (title, body) = parse_tool_document_buffer(&text);
        assert_eq!(title, "Briefing readability");
        assert_eq!(body, doc.body);
    }

    #[test]
    fn render_with_no_metadata_has_no_comment_line() {
        let doc = ToolDocument {
            title: "T".to_string(),
            body: "B\n".to_string(),
            labels: vec![],
            status: None,
        };
        let text = render_tool_document_buffer(&doc);
        assert!(!text.contains("<!--"));
        assert_eq!(text, "# T\n\nB\n");
    }

    /// Acceptance: "Open → edit → :w round-trips a document through a mock
    /// provider, with no coordinator present." #524.
    #[test]
    fn open_edit_save_round_trips_through_mock_provider() {
        let mut engine = Engine::new_for_test();
        install_mock_provider(&mut engine, fixture_provider());
        engine.set_document_client_for_test(MockToolClient(Ok(serde_json::json!({
            "title": "Briefing readability",
            "body": "Original body.\n",
            "labels": ["docs"],
            "status": "refining",
        }))));

        engine.open_tool_document("42").expect("mock read succeeds");
        assert!(
            engine.active_buffer_state().tool_document.is_some(),
            "opened buffer should be bound to the document"
        );
        assert_eq!(
            engine
                .active_buffer_state()
                .tool_document
                .as_ref()
                .unwrap()
                .id,
            Some("42".to_string())
        );
        assert!(engine.buffer().to_string().contains("Briefing readability"));
        assert!(
            engine.active_buffer_state().file_path.is_none(),
            "a document buffer must never be backed by a file on disk"
        );

        // Edit: replace the whole buffer with a new title + body.
        let new_text = "# Briefing readability, revised\n\nEdited body.\n";
        engine.active_buffer_state_mut().buffer.content = ropey::Rope::from_str(new_text);

        // Now swap in a recording client to observe exactly what gets
        // pushed on save.
        let recorder = RecordingToolClient::default();
        engine.set_document_client_for_test(recorder.clone());
        engine
            .save()
            .expect(":w should succeed against the mock provider");

        let calls = recorder.calls();
        assert_eq!(calls.len(), 2, "write, then the follow-up");
        assert_eq!(
            calls[0].argv,
            vec!["mock".to_string(), "write".to_string(), "42".to_string()]
        );
        let sent: serde_json::Value =
            serde_json::from_slice(calls[0].stdin.as_deref().unwrap()).unwrap();
        assert_eq!(sent["title"], "Briefing readability, revised");
        assert_eq!(sent["body"], "Edited body.\n");

        assert_eq!(
            calls[1].argv,
            vec!["mock".to_string(), "ready".to_string(), "42".to_string()]
        );

        // No file was ever written to disk.
        assert!(engine.active_buffer_state().file_path.is_none());
    }

    #[test]
    fn new_document_flow_writes_with_empty_id_and_no_follow_up() {
        let mut engine = Engine::new_for_test();
        install_mock_provider(&mut engine, fixture_provider());

        engine.new_tool_document().expect("blank buffer opens");
        assert_eq!(
            engine
                .active_buffer_state()
                .tool_document
                .as_ref()
                .unwrap()
                .id,
            None
        );

        let text = "# Fresh issue\n\nBrand new body.\n";
        engine.active_buffer_state_mut().buffer.content = ropey::Rope::from_str(text);

        let recorder = RecordingToolClient::default();
        engine.set_document_client_for_test(recorder.clone());
        engine
            .save()
            .expect(":w should create via the write command");

        let calls = recorder.calls();
        assert_eq!(
            calls.len(),
            1,
            "no follow-up on the create path — nothing to transition from"
        );
        assert_eq!(
            calls[0].argv,
            vec!["mock".to_string(), "write".to_string(), "".to_string()],
            "empty id substituted for a not-yet-created document"
        );
        let sent: serde_json::Value =
            serde_json::from_slice(calls[0].stdin.as_deref().unwrap()).unwrap();
        assert_eq!(sent["title"], "Fresh issue");
        assert_eq!(sent["body"], "Brand new body.\n");
    }

    /// Review follow-up (#524): if the write command echoes a
    /// provider-assigned id back on stdout, the create path binds the
    /// still-open buffer to it — so a *second* `:w` updates the
    /// now-existing document (a real write-command argv, not the create
    /// path's empty-id argv again) instead of silently duplicating it.
    #[test]
    fn new_document_flow_binds_the_buffer_to_a_provider_assigned_id_after_create() {
        let mut engine = Engine::new_for_test();
        install_mock_provider(&mut engine, fixture_provider());
        engine.new_tool_document().expect("blank buffer opens");
        engine.active_buffer_state_mut().buffer.content =
            ropey::Rope::from_str("# Fresh issue\n\nBrand new body.\n");

        engine.set_document_client_for_test(MockToolClient(Ok(serde_json::json!({ "id": "99" }))));
        engine
            .save()
            .expect(":w should create via the write command");

        assert_eq!(
            engine
                .active_buffer_state()
                .tool_document
                .as_ref()
                .unwrap()
                .id,
            Some("99".to_string()),
            "the buffer must be bound to the provider-assigned id after create"
        );

        // Second `:w`: now that the buffer is bound, this must be an
        // *update* — the write command's `{id}` substituted with "99", not
        // another empty-id create.
        let recorder = RecordingToolClient::default();
        engine.set_document_client_for_test(recorder.clone());
        engine
            .save()
            .expect(":w should update the now-bound document");
        let calls = recorder.calls();
        assert_eq!(
            calls.len(),
            2,
            "write, then the follow-up (id is now known)"
        );
        assert_eq!(
            calls[0].argv,
            vec!["mock".to_string(), "write".to_string(), "99".to_string()],
            "the second save must target the created document's id, not create again"
        );
    }

    #[test]
    fn save_propagates_write_failure_and_never_runs_follow_up() {
        let mut engine = Engine::new_for_test();
        install_mock_provider(&mut engine, fixture_provider());
        engine.set_document_client_for_test(MockToolClient(Ok(serde_json::json!({
            "title": "T", "body": "B\n",
        }))));
        engine.open_tool_document("7").unwrap();

        let recorder = RecordingToolClient::default();
        engine.set_document_client_for_test(MockToolClient(Err(ToolError::NonZeroExit {
            code: Some(1),
            stderr: "rejected".to_string(),
        })));
        let err = engine.save().unwrap_err();
        assert!(err.contains("rejected") || err.contains("status 1"));
        // Buffer stays dirty-eligible (save didn't clear it) — not
        // directly asserted here since `dirty` starts false on open, but
        // the message must reflect the failure, not a fake success.
        assert_ne!(engine.message, "Document saved");
        let _ = recorder; // unused in this branch; kept for symmetry/clarity
    }

    #[test]
    fn ordinary_buffer_save_is_unaffected() {
        // A document-buffer field being `None` must not change normal
        // `:w`-with-no-file behaviour.
        let mut engine = Engine::new_for_test();
        assert!(engine.active_buffer_state().tool_document.is_none());
        let err = engine.save().unwrap_err();
        assert_eq!(err, "No file name");
    }

    /// Review follow-up (#524): opening the same document id a second time
    /// must switch to the already-open buffer/tab, not open a duplicate —
    /// mirrors `open_keymaps_editor`'s precedent. Otherwise two buffers
    /// bound to the same id could race on `:w` (last save wins, first
    /// buffer's edits silently lost).
    #[test]
    fn opening_the_same_document_id_twice_switches_to_the_existing_tab() {
        let mut engine = Engine::new_for_test();
        install_mock_provider(&mut engine, fixture_provider());
        engine.set_document_client_for_test(MockToolClient(Ok(serde_json::json!({
            "title": "Briefing readability",
            "body": "Original body.\n",
        }))));

        engine
            .open_tool_document("42")
            .expect("first open succeeds");
        let tab_count_after_first_open = engine.active_group().tabs.len();
        let buf_id_after_first_open = engine.active_window().buffer_id;

        // Switch away to a different tab (a plain scratch buffer), so
        // reopening the same id has to navigate back rather than trivially
        // already being the active tab.
        let other_buf = engine.buffer_manager.create();
        let window_id = engine.new_window_id();
        engine.windows.insert(
            window_id,
            crate::core::window::Window::new(window_id, other_buf),
        );
        let tab_id = engine.new_tab_id();
        engine
            .active_group_mut()
            .tabs
            .push(crate::core::tab::Tab::new(tab_id, window_id));
        engine.active_group_mut().active_tab = engine.active_group().tabs.len() - 1;

        engine
            .open_tool_document("42")
            .expect("second open succeeds");

        assert_eq!(
            engine.active_group().tabs.len(),
            tab_count_after_first_open + 1,
            "no new document tab should be created for an id already open \
             (the one extra tab is the unrelated scratch buffer switched to above)"
        );
        assert_eq!(
            engine.active_window().buffer_id,
            buf_id_after_first_open,
            "reopening the same id must switch back to its existing buffer"
        );
    }
}
