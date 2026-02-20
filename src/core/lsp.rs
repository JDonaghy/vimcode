use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read as IoRead, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

pub type LspServerId = usize;

/// Events emitted by an LSP server, received via channel in the engine.
#[derive(Debug)]
#[allow(dead_code)] // server_id/request_id fields populated for future request correlation
pub enum LspEvent {
    Initialized(LspServerId),
    Diagnostics {
        server_id: LspServerId,
        path: PathBuf,
        diagnostics: Vec<Diagnostic>,
    },
    CompletionResponse {
        server_id: LspServerId,
        request_id: i64,
        items: Vec<CompletionItem>,
    },
    DefinitionResponse {
        server_id: LspServerId,
        request_id: i64,
        locations: Vec<Location>,
    },
    HoverResponse {
        server_id: LspServerId,
        request_id: i64,
        contents: Option<String>,
    },
    ServerExited(LspServerId),
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub range: LspRange,
    pub severity: DiagnosticSeverity,
    pub message: String,
    #[allow(dead_code)] // populated for future diagnostic source display
    pub source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

impl DiagnosticSeverity {
    pub fn from_lsp(value: i32) -> Self {
        match value {
            1 => DiagnosticSeverity::Error,
            2 => DiagnosticSeverity::Warning,
            3 => DiagnosticSeverity::Information,
            _ => DiagnosticSeverity::Hint,
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            DiagnosticSeverity::Error => "E",
            DiagnosticSeverity::Warning => "W",
            DiagnosticSeverity::Information => "I",
            DiagnosticSeverity::Hint => "H",
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // kind/detail/sort_text populated for future completion popup detail display
pub struct CompletionItem {
    pub label: String,
    pub kind: Option<String>,
    pub detail: Option<String>,
    pub insert_text: Option<String>,
    pub sort_text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Location {
    pub path: PathBuf,
    pub range: LspRange,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LspPosition {
    /// 0-indexed line number
    pub line: u32,
    /// 0-indexed UTF-16 code unit offset
    pub character: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LspServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub languages: Vec<String>,
}

// ---------------------------------------------------------------------------
// Pure helper functions
// ---------------------------------------------------------------------------

/// Encode a JSON-RPC message with Content-Length header.
pub fn encode_message(body: &str) -> Vec<u8> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut msg = header.into_bytes();
    msg.extend_from_slice(body.as_bytes());
    msg
}

/// Parse Content-Length from a header line. Returns None if not a valid header.
pub fn parse_content_length(line: &str) -> Option<usize> {
    let trimmed = line.trim();
    let value = trimmed.strip_prefix("Content-Length:")?;
    value.trim().parse().ok()
}

/// Convert a file path to a file:// URI.
pub fn path_to_uri(path: &Path) -> String {
    // Canonicalize to get absolute path, fall back to as-is
    let abs = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    format!("file://{}", abs.display())
}

/// Convert a file:// URI back to a PathBuf.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    uri.strip_prefix("file://").map(PathBuf::from)
}

/// Map a file extension to an LSP language identifier.
pub fn language_id_from_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    let lang = match ext {
        "rs" => "rust",
        "py" | "pyw" => "python",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "jsx" => "javascriptreact",
        "go" => "go",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => "cpp",
        "java" => "java",
        "rb" => "ruby",
        "lua" => "lua",
        "sh" | "bash" | "zsh" => "shellscript",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "html" | "htm" => "html",
        "css" => "css",
        "md" | "markdown" => "markdown",
        "zig" => "zig",
        "ex" | "exs" => "elixir",
        _ => return None,
    };
    Some(lang.to_string())
}

/// Convert a UTF-16 offset to a char (grapheme-unaware) column index within a line.
/// LSP positions use UTF-16 code units; Ropey/our engine use char indices.
pub fn utf16_offset_to_char(line_text: &str, utf16_offset: u32) -> usize {
    let mut utf16_count: u32 = 0;
    for (i, ch) in line_text.chars().enumerate() {
        if utf16_count >= utf16_offset {
            return i;
        }
        utf16_count += ch.len_utf16() as u32;
    }
    // Offset past end of line — clamp to line length
    line_text.chars().count()
}

/// Convert a char column index to a UTF-16 offset.
pub fn char_to_utf16_offset(line_text: &str, char_idx: usize) -> u32 {
    line_text
        .chars()
        .take(char_idx)
        .map(|ch| ch.len_utf16() as u32)
        .sum()
}

/// Completion item kind number to a human-readable short string.
pub fn completion_kind_label(kind: u32) -> &'static str {
    match kind {
        1 => "Text",
        2 => "Method",
        3 => "Function",
        4 => "Constructor",
        5 => "Field",
        6 => "Variable",
        7 => "Class",
        8 => "Interface",
        9 => "Module",
        10 => "Property",
        11 => "Unit",
        12 => "Value",
        13 => "Enum",
        14 => "Keyword",
        15 => "Snippet",
        16 => "Color",
        17 => "File",
        18 => "Reference",
        19 => "Folder",
        20 => "EnumMember",
        21 => "Constant",
        22 => "Struct",
        23 => "Event",
        24 => "Operator",
        25 => "TypeParameter",
        _ => "Unknown",
    }
}

// ---------------------------------------------------------------------------
// LspServer — manages a single language server subprocess
// ---------------------------------------------------------------------------

pub struct LspServer {
    #[allow(dead_code)] // stored for future server-identification in multi-server scenarios
    id: LspServerId,
    #[allow(dead_code)]
    config: LspServerConfig,
    stdin: Arc<Mutex<Box<dyn IoWrite + Send>>>,
    next_request_id: i64,
    #[allow(dead_code)]
    child: Child,
    document_versions: HashMap<String, i32>,
    /// Maps request IDs to method names so the reader thread can route responses.
    pending_requests: Arc<Mutex<HashMap<i64, String>>>,
}

impl LspServer {
    /// Start a new language server process. Sends `initialize` and waits for
    /// the response on a background thread, then sends `initialized`.
    pub fn start(
        id: LspServerId,
        config: &LspServerConfig,
        root_path: &Path,
        event_tx: Sender<LspEvent>,
    ) -> Result<Self, String> {
        let mut child = Command::new(&config.command)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to start {}: {}", config.command, e))?;

        let stdout = child.stdout.take().ok_or("Failed to get server stdout")?;
        let stdin: Box<dyn IoWrite + Send> =
            Box::new(child.stdin.take().ok_or("Failed to get server stdin")?);
        let stdin = Arc::new(Mutex::new(stdin));
        let pending_requests: Arc<Mutex<HashMap<i64, String>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Start the reader thread
        let reader_server_id = id;
        let reader_tx = event_tx;
        let reader_pending = pending_requests.clone();
        let reader_stdin = stdin.clone();
        thread::spawn(move || {
            reader_thread(
                stdout,
                reader_tx,
                reader_server_id,
                reader_pending,
                reader_stdin,
            );
        });

        let mut server = Self {
            id,
            config: config.clone(),
            stdin,
            next_request_id: 1,
            child,
            document_versions: HashMap::new(),
            pending_requests,
        };

        // Send initialize request
        let root_uri = path_to_uri(root_path);
        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "completion": {
                        "completionItem": {
                            "snippetSupport": false,
                            "labelDetailsSupport": true
                        }
                    },
                    "hover": {
                        "contentFormat": ["plaintext", "markdown"]
                    },
                    "publishDiagnostics": {
                        "relatedInformation": false
                    },
                    "definition": {},
                    "synchronization": {
                        "didSave": true,
                        "willSave": false,
                        "willSaveWaitUntil": false
                    }
                }
            }
        });
        server.send_request("initialize", init_params);

        Ok(server)
    }

    fn send_request(&mut self, method: &str, params: serde_json::Value) -> i64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        // Record the request so the reader thread can route the response
        if let Ok(mut pending) = self.pending_requests.lock() {
            pending.insert(id, method.to_string());
        }
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.send_raw(&msg.to_string());
        id
    }

    fn send_notification(&self, method: &str, params: serde_json::Value) {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.send_raw(&msg.to_string());
    }

    fn send_raw(&self, body: &str) {
        let encoded = encode_message(body);
        if let Ok(mut stdin) = self.stdin.lock() {
            let _ = stdin.write_all(&encoded);
            let _ = stdin.flush();
        }
    }

    /// Notify the server that a document was opened.
    pub fn did_open(&mut self, uri: &str, language_id: &str, text: &str) {
        let version = 1;
        self.document_versions.insert(uri.to_string(), version);
        self.send_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": version,
                    "text": text
                }
            }),
        );
    }

    /// Notify the server that a document changed (full sync).
    pub fn did_change(&mut self, uri: &str, text: &str) {
        let version = self.document_versions.entry(uri.to_string()).or_insert(0);
        *version += 1;
        let v = *version;
        self.send_notification(
            "textDocument/didChange",
            serde_json::json!({
                "textDocument": { "uri": uri, "version": v },
                "contentChanges": [{ "text": text }]
            }),
        );
    }

    /// Notify the server that a document was saved.
    pub fn did_save(&self, uri: &str, text: &str) {
        self.send_notification(
            "textDocument/didSave",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "text": text
            }),
        );
    }

    /// Notify the server that a document was closed.
    pub fn did_close(&mut self, uri: &str) {
        self.document_versions.remove(uri);
        self.send_notification(
            "textDocument/didClose",
            serde_json::json!({
                "textDocument": { "uri": uri }
            }),
        );
    }

    /// Request completions at a position.
    pub fn request_completion(&mut self, uri: &str, line: u32, character: u32) -> i64 {
        self.send_request(
            "textDocument/completion",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
    }

    /// Request go-to-definition at a position.
    pub fn request_definition(&mut self, uri: &str, line: u32, character: u32) -> i64 {
        self.send_request(
            "textDocument/definition",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
    }

    /// Request hover info at a position.
    pub fn request_hover(&mut self, uri: &str, line: u32, character: u32) -> i64 {
        self.send_request(
            "textDocument/hover",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
    }

    /// Send shutdown request and exit notification.
    pub fn shutdown(&mut self) {
        self.send_request("shutdown", serde_json::json!(null));
        self.send_notification("exit", serde_json::json!(null));
    }

    #[allow(dead_code)]
    pub fn server_id(&self) -> LspServerId {
        self.id
    }
}

// ---------------------------------------------------------------------------
// Reader thread — runs on background thread, reads server stdout
// ---------------------------------------------------------------------------

fn reader_thread(
    stdout: impl IoRead + Send + 'static,
    tx: Sender<LspEvent>,
    server_id: LspServerId,
    pending_requests: Arc<Mutex<HashMap<i64, String>>>,
    stdin: Arc<Mutex<Box<dyn IoWrite + Send>>>,
) {
    let mut reader = BufReader::new(stdout);
    let mut header_buf = String::new();
    let mut initialized_sent = false;

    loop {
        // Read headers until blank line
        let mut content_length: Option<usize> = None;
        loop {
            header_buf.clear();
            match reader.read_line(&mut header_buf) {
                Ok(0) => {
                    // EOF — server exited
                    let _ = tx.send(LspEvent::ServerExited(server_id));
                    return;
                }
                Ok(_) => {
                    let trimmed = header_buf.trim();
                    if trimmed.is_empty() {
                        break; // End of headers
                    }
                    if let Some(len) = parse_content_length(trimmed) {
                        content_length = Some(len);
                    }
                }
                Err(_) => {
                    let _ = tx.send(LspEvent::ServerExited(server_id));
                    return;
                }
            }
        }

        let content_length = match content_length {
            Some(len) => len,
            None => continue, // Malformed message, skip
        };

        // Read body
        let mut body = vec![0u8; content_length];
        if reader.read_exact(&mut body).is_err() {
            let _ = tx.send(LspEvent::ServerExited(server_id));
            return;
        }

        let body_str = match String::from_utf8(body) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let json: serde_json::Value = match serde_json::from_str(&body_str) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Handle server → client messages that have a "method" field.
        if let Some(method) = json.get("method").and_then(|m| m.as_str()) {
            // Server-initiated requests have both "method" and "id" — respond to them.
            if let Some(req_id) = json.get("id") {
                let response = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": null
                });
                let body = response.to_string();
                let encoded = encode_message(&body);
                if let Ok(mut w) = stdin.lock() {
                    let _ = w.write_all(&encoded);
                    let _ = w.flush();
                }
            } else if method == "textDocument/publishDiagnostics" {
                // Pure notification — parse diagnostics.
                if let Some(params) = json.get("params") {
                    if let Some(event) = parse_diagnostics(server_id, params) {
                        let _ = tx.send(event);
                    }
                }
            }
            continue;
        }

        // Handle responses to our requests (has "id")
        if let Some(id) = json.get("id").and_then(|v| v.as_i64()) {
            // Look up which method this response is for
            let method = pending_requests
                .lock()
                .ok()
                .and_then(|mut map| map.remove(&id));

            let result = json.get("result");
            let is_error = json.get("error").is_some();

            match method.as_deref() {
                Some("initialize") => {
                    if !initialized_sent {
                        if let Some(r) = result {
                            if r.get("capabilities").is_some() {
                                initialized_sent = true;
                                let _ = tx.send(LspEvent::Initialized(server_id));
                            }
                        }
                    }
                }
                Some("textDocument/completion") => {
                    if let Some(r) = result {
                        if let Some(event) = try_parse_completion_response(server_id, id, r) {
                            let _ = tx.send(event);
                        }
                    } else if is_error {
                        let _ = tx.send(LspEvent::CompletionResponse {
                            server_id,
                            request_id: id,
                            items: Vec::new(),
                        });
                    }
                }
                Some("textDocument/definition") => {
                    if let Some(r) = result {
                        if let Some(event) = try_parse_definition_response(server_id, id, r) {
                            let _ = tx.send(event);
                        }
                    } else if is_error {
                        let _ = tx.send(LspEvent::DefinitionResponse {
                            server_id,
                            request_id: id,
                            locations: Vec::new(),
                        });
                    }
                }
                Some("textDocument/hover") => {
                    if let Some(r) = result {
                        if let Some(event) = try_parse_hover_response(server_id, id, r) {
                            let _ = tx.send(event);
                        }
                    } else if is_error {
                        let _ = tx.send(LspEvent::HoverResponse {
                            server_id,
                            request_id: id,
                            contents: None,
                        });
                    }
                }
                _ => {
                    // Unknown or shutdown response — ignore
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Response parsers
// ---------------------------------------------------------------------------

fn parse_diagnostics(server_id: LspServerId, params: &serde_json::Value) -> Option<LspEvent> {
    let uri = params.get("uri")?.as_str()?;
    let path = uri_to_path(uri)?;
    let diags_array = params.get("diagnostics")?.as_array()?;
    let mut diagnostics = Vec::new();

    for d in diags_array {
        let range = parse_range(d.get("range")?)?;
        let severity = d
            .get("severity")
            .and_then(|s| s.as_i64())
            .map(|s| DiagnosticSeverity::from_lsp(s as i32))
            .unwrap_or(DiagnosticSeverity::Error);
        let message = d.get("message")?.as_str()?.to_string();
        let source = d
            .get("source")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        diagnostics.push(Diagnostic {
            range,
            severity,
            message,
            source,
        });
    }

    Some(LspEvent::Diagnostics {
        server_id,
        path,
        diagnostics,
    })
}

fn try_parse_completion_response(
    server_id: LspServerId,
    request_id: i64,
    result: &serde_json::Value,
) -> Option<LspEvent> {
    // Completions can be an array or { isIncomplete, items: [] }
    let items_array = if result.is_array() {
        result.as_array()?
    } else {
        result.get("items")?.as_array()?
    };

    let mut items = Vec::new();
    for item in items_array {
        let label = item.get("label")?.as_str()?.to_string();
        let kind = item
            .get("kind")
            .and_then(|k| k.as_u64())
            .map(|k| completion_kind_label(k as u32).to_string());
        let detail = item
            .get("detail")
            .and_then(|d| d.as_str())
            .map(|s| s.to_string());
        let insert_text = item
            .get("insertText")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());
        let sort_text = item
            .get("sortText")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());
        items.push(CompletionItem {
            label,
            kind,
            detail,
            insert_text,
            sort_text,
        });
    }

    Some(LspEvent::CompletionResponse {
        server_id,
        request_id,
        items,
    })
}

fn try_parse_definition_response(
    server_id: LspServerId,
    request_id: i64,
    result: &serde_json::Value,
) -> Option<LspEvent> {
    let mut locations = Vec::new();

    if result.is_array() {
        // Array of Location or LocationLink
        for loc in result.as_array()? {
            if let Some(l) = parse_location(loc) {
                locations.push(l);
            } else if let Some(l) = parse_location_link(loc) {
                locations.push(l);
            }
        }
    } else if result.is_object() {
        // Single Location
        if let Some(l) = parse_location(result) {
            locations.push(l);
        } else if let Some(l) = parse_location_link(result) {
            locations.push(l);
        }
    } else if result.is_null() {
        // No definition found
    } else {
        return None;
    }

    Some(LspEvent::DefinitionResponse {
        server_id,
        request_id,
        locations,
    })
}

fn try_parse_hover_response(
    server_id: LspServerId,
    request_id: i64,
    result: &serde_json::Value,
) -> Option<LspEvent> {
    if result.is_null() {
        return Some(LspEvent::HoverResponse {
            server_id,
            request_id,
            contents: None,
        });
    }

    let contents = result.get("contents")?;
    let text = extract_markup_content(contents);

    Some(LspEvent::HoverResponse {
        server_id,
        request_id,
        contents: text,
    })
}

fn extract_markup_content(value: &serde_json::Value) -> Option<String> {
    // MarkupContent { kind, value }
    if let Some(val) = value.get("value").and_then(|v| v.as_str()) {
        return Some(val.to_string());
    }
    // Plain string
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    // Array of MarkedString
    if let Some(arr) = value.as_array() {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|v| {
                v.as_str().map(|s| s.to_string()).or_else(|| {
                    v.get("value")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
            })
            .collect();
        if !parts.is_empty() {
            return Some(parts.join("\n\n"));
        }
    }
    None
}

fn parse_location(value: &serde_json::Value) -> Option<Location> {
    let uri = value.get("uri")?.as_str()?;
    let path = uri_to_path(uri)?;
    let range = parse_range(value.get("range")?)?;
    Some(Location { path, range })
}

fn parse_location_link(value: &serde_json::Value) -> Option<Location> {
    let uri = value.get("targetUri")?.as_str()?;
    let path = uri_to_path(uri)?;
    let range = parse_range(
        value
            .get("targetSelectionRange")
            .or(value.get("targetRange"))?,
    )?;
    Some(Location { path, range })
}

fn parse_range(value: &serde_json::Value) -> Option<LspRange> {
    let start = value.get("start")?;
    let end = value.get("end")?;
    Some(LspRange {
        start: LspPosition {
            line: start.get("line")?.as_u64()? as u32,
            character: start.get("character")?.as_u64()? as u32,
        },
        end: LspPosition {
            line: end.get("line")?.as_u64()? as u32,
            character: end.get("character")?.as_u64()? as u32,
        },
    })
}

// ---------------------------------------------------------------------------
// Handle the "initialized" notification — called by LspManager when
// it receives an LspEvent::Initialized.
// ---------------------------------------------------------------------------

impl LspServer {
    /// Send the `initialized` notification after the server has responded
    /// to our `initialize` request. Must be called exactly once.
    pub fn send_initialized(&self) {
        self.send_notification("initialized", serde_json::json!({}));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_message() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let encoded = encode_message(body);
        let s = String::from_utf8(encoded).unwrap();
        assert!(s.starts_with("Content-Length: 46\r\n\r\n"));
        assert!(s.ends_with(body));
    }

    #[test]
    fn test_encode_message_empty() {
        let encoded = encode_message("");
        let s = String::from_utf8(encoded).unwrap();
        assert_eq!(s, "Content-Length: 0\r\n\r\n");
    }

    #[test]
    fn test_parse_content_length_valid() {
        assert_eq!(parse_content_length("Content-Length: 42"), Some(42));
        assert_eq!(parse_content_length("Content-Length:42"), Some(42));
        assert_eq!(parse_content_length("  Content-Length: 100  "), Some(100));
    }

    #[test]
    fn test_parse_content_length_invalid() {
        assert_eq!(parse_content_length("Content-Type: text/plain"), None);
        assert_eq!(parse_content_length("Content-Length: abc"), None);
        assert_eq!(parse_content_length(""), None);
    }

    #[test]
    fn test_path_to_uri() {
        let uri = path_to_uri(Path::new("/home/user/file.rs"));
        assert!(uri.starts_with("file://"));
        assert!(uri.contains("file.rs"));
    }

    #[test]
    fn test_uri_to_path() {
        let path = uri_to_path("file:///home/user/file.rs");
        assert_eq!(path, Some(PathBuf::from("/home/user/file.rs")));
    }

    #[test]
    fn test_uri_to_path_invalid() {
        assert_eq!(uri_to_path("http://example.com"), None);
        assert_eq!(uri_to_path("foobar"), None);
    }

    #[test]
    fn test_language_id_from_path() {
        assert_eq!(
            language_id_from_path(Path::new("main.rs")),
            Some("rust".to_string())
        );
        assert_eq!(
            language_id_from_path(Path::new("app.py")),
            Some("python".to_string())
        );
        assert_eq!(
            language_id_from_path(Path::new("index.js")),
            Some("javascript".to_string())
        );
        assert_eq!(
            language_id_from_path(Path::new("main.go")),
            Some("go".to_string())
        );
        assert_eq!(
            language_id_from_path(Path::new("main.cpp")),
            Some("cpp".to_string())
        );
        assert_eq!(
            language_id_from_path(Path::new("main.c")),
            Some("c".to_string())
        );
        assert_eq!(
            language_id_from_path(Path::new("app.ts")),
            Some("typescript".to_string())
        );
        assert_eq!(language_id_from_path(Path::new("Makefile")), None);
        assert_eq!(language_id_from_path(Path::new("file.xyz")), None);
    }

    #[test]
    fn test_utf16_offset_to_char_ascii() {
        // ASCII: UTF-16 offset == char index
        assert_eq!(utf16_offset_to_char("hello world", 0), 0);
        assert_eq!(utf16_offset_to_char("hello world", 5), 5);
        assert_eq!(utf16_offset_to_char("hello world", 11), 11);
    }

    #[test]
    fn test_utf16_offset_to_char_multibyte() {
        // '€' is U+20AC, encoded as 1 UTF-16 code unit but 3 UTF-8 bytes
        // "a€b" — chars: a(0), €(1), b(2)
        // UTF-16 offsets: a=0, €=1, b=2
        assert_eq!(utf16_offset_to_char("a€b", 0), 0);
        assert_eq!(utf16_offset_to_char("a€b", 1), 1);
        assert_eq!(utf16_offset_to_char("a€b", 2), 2);

        // '𝄞' (U+1D11E) is 2 UTF-16 code units (surrogate pair)
        // "a𝄞b" — chars: a(0), 𝄞(1), b(2)
        // UTF-16 offsets: a=0, 𝄞=1..2, b=3
        assert_eq!(utf16_offset_to_char("a𝄞b", 0), 0);
        assert_eq!(utf16_offset_to_char("a𝄞b", 1), 1);
        assert_eq!(utf16_offset_to_char("a𝄞b", 3), 2);
    }

    #[test]
    fn test_char_to_utf16_offset_ascii() {
        assert_eq!(char_to_utf16_offset("hello", 0), 0);
        assert_eq!(char_to_utf16_offset("hello", 3), 3);
        assert_eq!(char_to_utf16_offset("hello", 5), 5);
    }

    #[test]
    fn test_char_to_utf16_offset_multibyte() {
        // '𝄞' is 2 UTF-16 code units
        assert_eq!(char_to_utf16_offset("a𝄞b", 0), 0);
        assert_eq!(char_to_utf16_offset("a𝄞b", 1), 1);
        assert_eq!(char_to_utf16_offset("a𝄞b", 2), 3); // after surrogate pair
    }

    #[test]
    fn test_utf16_roundtrip() {
        let text = "hello";
        for i in 0..=text.len() {
            let utf16 = char_to_utf16_offset(text, i);
            let back = utf16_offset_to_char(text, utf16);
            assert_eq!(back, i, "roundtrip failed for char_idx={i}");
        }
    }

    #[test]
    fn test_completion_kind_label() {
        assert_eq!(completion_kind_label(3), "Function");
        assert_eq!(completion_kind_label(6), "Variable");
        assert_eq!(completion_kind_label(22), "Struct");
        assert_eq!(completion_kind_label(99), "Unknown");
    }

    #[test]
    fn test_diagnostic_severity_from_lsp() {
        assert_eq!(DiagnosticSeverity::from_lsp(1), DiagnosticSeverity::Error);
        assert_eq!(DiagnosticSeverity::from_lsp(2), DiagnosticSeverity::Warning);
        assert_eq!(
            DiagnosticSeverity::from_lsp(3),
            DiagnosticSeverity::Information
        );
        assert_eq!(DiagnosticSeverity::from_lsp(4), DiagnosticSeverity::Hint);
        assert_eq!(DiagnosticSeverity::from_lsp(99), DiagnosticSeverity::Hint);
    }

    #[test]
    fn test_diagnostic_severity_symbol() {
        assert_eq!(DiagnosticSeverity::Error.symbol(), "E");
        assert_eq!(DiagnosticSeverity::Warning.symbol(), "W");
        assert_eq!(DiagnosticSeverity::Hint.symbol(), "H");
    }

    #[test]
    fn test_parse_diagnostics_json() {
        let params = serde_json::json!({
            "uri": "file:///home/user/main.rs",
            "diagnostics": [
                {
                    "range": {
                        "start": { "line": 5, "character": 10 },
                        "end": { "line": 5, "character": 15 }
                    },
                    "severity": 1,
                    "message": "expected type `u32`",
                    "source": "rust-analyzer"
                }
            ]
        });
        let event = parse_diagnostics(0, &params).unwrap();
        match event {
            LspEvent::Diagnostics {
                path, diagnostics, ..
            } => {
                assert_eq!(path, PathBuf::from("/home/user/main.rs"));
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].message, "expected type `u32`");
                assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
                assert_eq!(diagnostics[0].range.start.line, 5);
                assert_eq!(diagnostics[0].range.start.character, 10);
            }
            _ => panic!("Expected Diagnostics event"),
        }
    }

    #[test]
    fn test_parse_hover_response() {
        let result = serde_json::json!({
            "contents": {
                "kind": "plaintext",
                "value": "fn main()"
            }
        });
        let event = try_parse_hover_response(0, 1, &result).unwrap();
        match event {
            LspEvent::HoverResponse { contents, .. } => {
                assert_eq!(contents, Some("fn main()".to_string()));
            }
            _ => panic!("Expected HoverResponse"),
        }
    }

    #[test]
    fn test_parse_hover_response_null() {
        let result = serde_json::json!(null);
        let event = try_parse_hover_response(0, 1, &result).unwrap();
        match event {
            LspEvent::HoverResponse { contents, .. } => {
                assert!(contents.is_none());
            }
            _ => panic!("Expected HoverResponse"),
        }
    }

    #[test]
    fn test_parse_definition_response_single() {
        let result = serde_json::json!({
            "uri": "file:///home/user/lib.rs",
            "range": {
                "start": { "line": 10, "character": 4 },
                "end": { "line": 10, "character": 12 }
            }
        });
        let event = try_parse_definition_response(0, 1, &result).unwrap();
        match event {
            LspEvent::DefinitionResponse { locations, .. } => {
                assert_eq!(locations.len(), 1);
                assert_eq!(locations[0].path, PathBuf::from("/home/user/lib.rs"));
                assert_eq!(locations[0].range.start.line, 10);
            }
            _ => panic!("Expected DefinitionResponse"),
        }
    }

    #[test]
    fn test_parse_definition_response_null() {
        let result = serde_json::json!(null);
        let event = try_parse_definition_response(0, 1, &result).unwrap();
        match event {
            LspEvent::DefinitionResponse { locations, .. } => {
                assert!(locations.is_empty());
            }
            _ => panic!("Expected DefinitionResponse"),
        }
    }

    #[test]
    fn test_parse_completion_response_array() {
        let result = serde_json::json!([
            { "label": "foo", "kind": 3, "detail": "fn foo()" },
            { "label": "bar", "kind": 6 }
        ]);
        let event = try_parse_completion_response(0, 1, &result).unwrap();
        match event {
            LspEvent::CompletionResponse { items, .. } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].label, "foo");
                assert_eq!(items[0].kind, Some("Function".to_string()));
                assert_eq!(items[0].detail, Some("fn foo()".to_string()));
                assert_eq!(items[1].label, "bar");
                assert_eq!(items[1].kind, Some("Variable".to_string()));
            }
            _ => panic!("Expected CompletionResponse"),
        }
    }

    #[test]
    fn test_parse_completion_response_object() {
        let result = serde_json::json!({
            "isIncomplete": false,
            "items": [
                { "label": "baz", "kind": 22 }
            ]
        });
        let event = try_parse_completion_response(0, 1, &result).unwrap();
        match event {
            LspEvent::CompletionResponse { items, .. } => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].label, "baz");
                assert_eq!(items[0].kind, Some("Struct".to_string()));
            }
            _ => panic!("Expected CompletionResponse"),
        }
    }

    #[test]
    fn test_parse_location_link() {
        let result = serde_json::json!([{
            "targetUri": "file:///src/lib.rs",
            "targetRange": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 5, "character": 0 }
            },
            "targetSelectionRange": {
                "start": { "line": 2, "character": 4 },
                "end": { "line": 2, "character": 10 }
            }
        }]);
        let event = try_parse_definition_response(0, 1, &result).unwrap();
        match event {
            LspEvent::DefinitionResponse { locations, .. } => {
                assert_eq!(locations.len(), 1);
                // Should prefer targetSelectionRange
                assert_eq!(locations[0].range.start.line, 2);
                assert_eq!(locations[0].range.start.character, 4);
            }
            _ => panic!("Expected DefinitionResponse"),
        }
    }

    #[test]
    fn test_extract_markup_content_string() {
        let v = serde_json::json!("simple text");
        assert_eq!(extract_markup_content(&v), Some("simple text".to_string()));
    }

    #[test]
    fn test_extract_markup_content_object() {
        let v = serde_json::json!({"kind": "markdown", "value": "# Hello"});
        assert_eq!(extract_markup_content(&v), Some("# Hello".to_string()));
    }

    #[test]
    fn test_extract_markup_content_array() {
        let v = serde_json::json!(["first", {"language": "rust", "value": "fn main()"}]);
        let result = extract_markup_content(&v).unwrap();
        assert!(result.contains("first"));
        assert!(result.contains("fn main()"));
    }
}
