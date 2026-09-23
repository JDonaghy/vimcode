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
//! ## Standing commitments (apply to the whole ACP track, not just this file)
//!
//! - Protocol version is pinned at `1` (a draft v2 restructures capabilities
//!   and drops `fs/*`/`terminal/*` in favour of MCP-over-ACP).
//! - Capabilities advertised in `initialize` are **only** what later slices
//!   actually implement. Omitted means unsupported. This slice advertises
//!   nothing beyond the mandatory surface (`clientCapabilities: {}`).
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

    /// Send the `initialize` request. Advertises no `clientCapabilities`
    /// beyond the mandatory surface — see the module doc's capability
    /// negotiation commitment.
    pub fn initialize(&mut self) -> i64 {
        self.send_request(
            "initialize",
            serde_json::json!({
                "protocolVersion": PROTOCOL_VERSION,
                "clientCapabilities": {},
                "clientInfo": {
                    "name": "vimcode",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )
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
