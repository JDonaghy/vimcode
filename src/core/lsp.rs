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
    Initialized(LspServerId, serde_json::Value),
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
    /// Background registry lookup result (from Mason registry fetch).
    RegistryLookup {
        lang_id: String,
        info: Option<MasonPackageInfo>,
    },
    /// Background install command completed.
    InstallComplete {
        lang_id: String,
        success: bool,
        output: String,
    },
    /// References response (textDocument/references).
    ReferencesResponse {
        server_id: LspServerId,
        request_id: i64,
        locations: Vec<Location>,
    },
    /// Go-to-implementation response (textDocument/implementation).
    ImplementationResponse {
        server_id: LspServerId,
        request_id: i64,
        locations: Vec<Location>,
    },
    /// Go-to-type-definition response (textDocument/typeDefinition).
    TypeDefinitionResponse {
        server_id: LspServerId,
        request_id: i64,
        locations: Vec<Location>,
    },
    /// Signature help response (textDocument/signatureHelp).
    SignatureHelpResponse {
        server_id: LspServerId,
        request_id: i64,
        /// Full label of the first/active signature, e.g. "fn foo(a: i32, b: &str) -> bool"
        label: String,
        /// Byte-offset ranges of each parameter within `label`.
        params: Vec<(usize, usize)>,
        /// Index of the currently active parameter (0-based).
        active_param: Option<usize>,
    },
    /// Formatting edits response (textDocument/formatting or rangeFormatting).
    FormattingResponse {
        server_id: LspServerId,
        request_id: i64,
        edits: Vec<FormattingEdit>,
    },
    /// Rename response (textDocument/rename).
    RenameResponse {
        server_id: LspServerId,
        request_id: i64,
        workspace_edit: WorkspaceEdit,
        /// Error message from the server, if the response contained an error.
        error_message: Option<String>,
    },
}

/// Cached signature help data stored in engine state.
#[derive(Debug, Clone)]
pub struct SignatureHelpData {
    pub label: String,
    pub params: Vec<(usize, usize)>,
    pub active_param: Option<usize>,
}

/// A single text edit produced by formatting or rename operations.
#[derive(Debug, Clone)]
pub struct FormattingEdit {
    pub range: LspRange,
    pub new_text: String,
}

/// A set of edits for a single file.
#[derive(Debug, Clone)]
pub struct FileEdit {
    pub path: PathBuf,
    pub edits: Vec<FormattingEdit>,
}

/// A workspace-wide set of edits (rename result).
#[derive(Debug, Clone)]
pub struct WorkspaceEdit {
    pub changes: Vec<FileEdit>,
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

/// Map a file extension (or filename) to an LSP language identifier.
/// `user_map` (from `settings.language_map`) is checked first, allowing overrides
/// like `{ "h": "cpp", "mjs": "javascript" }`.
pub fn language_id_from_path_with_map(
    path: &Path,
    user_map: &std::collections::HashMap<String, String>,
) -> Option<String> {
    // User overrides by extension take priority
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        if let Some(lang) = user_map.get(ext) {
            return Some(lang.clone());
        }
    }
    language_id_from_path(path)
}

/// Map a file extension (or filename) to an LSP language identifier.
pub fn language_id_from_path(path: &Path) -> Option<String> {
    // Filename-only matches (no extension)
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name == "Dockerfile" || name.starts_with("Dockerfile.") {
            return Some("dockerfile".to_string());
        }
    }

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
        "cs" => "csharp",
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
        "kt" | "kts" => "kotlin",
        "php" => "php",
        "hs" | "lhs" => "haskell",
        "ml" | "mli" => "ocaml",
        "nix" => "nix",
        "tf" | "tfvars" => "terraform",
        "scala" | "sc" => "scala",
        "graphql" | "gql" => "graphql",
        "sql" => "sql",
        "sol" => "solidity",
        "swift" => "swift",
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
// Mason registry helpers (kept for reference; no longer actively used)
// ---------------------------------------------------------------------------

/// Information parsed from a Mason package.yaml.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MasonPackageInfo {
    /// Binary names that the package installs (from the `bin:` section).
    pub binaries: Vec<String>,
    /// Shell command to install this package (derived from PURL `source.id`).
    pub install_cmd: Option<String>,
    /// Package categories from the `categories:` YAML section, e.g. ["LSP"], ["DAP"], ["Linter"].
    pub categories: Vec<String>,
}

#[allow(dead_code)]
impl MasonPackageInfo {
    pub fn is_lsp(&self) -> bool {
        self.categories.iter().any(|c| c == "LSP")
    }
    pub fn is_dap(&self) -> bool {
        self.categories.iter().any(|c| c == "DAP")
    }
    pub fn is_linter(&self) -> bool {
        self.categories.iter().any(|c| c == "Linter")
    }
    pub fn is_formatter(&self) -> bool {
        self.categories.iter().any(|c| c == "Formatter")
    }
}

/// Static mapping: LSP language ID → Mason registry package name.
/// Kept for reference; no longer actively called.
#[allow(dead_code)]
pub fn mason_package_for_language(lang_id: &str) -> Option<&'static str> {
    match lang_id {
        "csharp" => Some("csharp-language-server"),
        "lua" => Some("lua-language-server"),
        "shellscript" => Some("bash-language-server"),
        "yaml" => Some("yaml-language-server"),
        "html" => Some("html-lsp"),
        "css" => Some("css-lsp"),
        "json" => Some("json-lsp"),
        "ruby" => Some("ruby-lsp"),
        "kotlin" => Some("kotlin-language-server"),
        "php" => Some("intelephense"),
        "elixir" => Some("elixir-ls"),
        "zig" => Some("zls"),
        "haskell" => Some("haskell-language-server"),
        "ocaml" => Some("ocaml-lsp"),
        "nix" => Some("nil"),
        "terraform" => Some("terraform-ls"),
        "java" => Some("jdtls"),
        "markdown" => Some("marksman"),
        "toml" => Some("taplo"),
        "dockerfile" => Some("dockerfile-language-server"),
        "graphql" => Some("graphql-language-service-cli"),
        "sql" => Some("sqls"),
        "solidity" => Some("nomicfoundation-solidity-language-server"),
        // scala and swift are not in Mason — PATH detection only
        _ => None,
    }
}

/// Convert a PURL string (e.g. `pkg:npm/bash-language-server`) to a shell install command.
#[allow(dead_code)]
pub fn parse_purl_install_cmd(purl: &str) -> Option<String> {
    let purl = purl.trim();
    if let Some(rest) = purl.strip_prefix("pkg:npm/") {
        let name = rest.split('@').next()?.trim();
        return Some(format!("npm install -g {name}"));
    }
    if let Some(rest) = purl.strip_prefix("pkg:nuget/") {
        let name = rest.split('@').next()?.trim();
        return Some(format!("dotnet tool install -g {name}"));
    }
    if let Some(rest) = purl.strip_prefix("pkg:golang/") {
        let module = rest.trim();
        // Mason golang PURLs often end with a version like @v0.x.y — strip it for @latest
        let module = module.split('@').next()?.trim();
        return Some(format!("go install {module}@latest"));
    }
    if let Some(rest) = purl.strip_prefix("pkg:pypi/") {
        let name = rest.split('@').next()?.trim();
        return Some(format!("pip install {name}"));
    }
    if let Some(rest) = purl.strip_prefix("pkg:cargo/") {
        let name = rest.split('@').next()?.trim();
        return Some(format!("cargo install {name}"));
    }
    // pkg:github/..., pkg:generic/... — no automated install
    None
}

/// Parse the relevant parts of a Mason `package.yaml` file.
/// This is a minimal hand-written parser — no YAML crate needed.
///
/// We extract:
/// - Binary names from the `bin:` section (indented key lines).
/// - Install command from the first `id: pkg:` line under `source:`.
/// - Package categories from the `categories:` section (e.g. LSP, DAP, Linter).
#[allow(dead_code)]
pub fn parse_mason_package_yaml(yaml: &str) -> MasonPackageInfo {
    let mut binaries = Vec::new();
    let mut install_cmd = None;
    let mut categories = Vec::new();

    let mut in_bin_section = false;
    let mut in_source_section = false;
    let mut in_categories_section = false;

    for line in yaml.lines() {
        let trimmed = line.trim();

        // Track which top-level section we are in
        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_bin_section = trimmed == "bin:";
            in_source_section = trimmed == "source:";
            in_categories_section = trimmed == "categories:";
            continue;
        }

        if in_categories_section {
            // Lines like `  - LSP` or `  - DAP`
            if let Some(rest) = trimmed.strip_prefix("- ") {
                let cat = rest.trim().trim_matches('"').trim_matches('\'');
                if !cat.is_empty() {
                    categories.push(cat.to_string());
                }
            }
        }

        if in_bin_section {
            // Lines like `  binary-name: path/to/binary` or `  binary-name:`
            // We only want the key (binary name).
            if let Some(key) = trimmed.split(':').next() {
                let key = key.trim().trim_matches('"').trim_matches('\'');
                if !key.is_empty() && !key.starts_with('#') {
                    binaries.push(key.to_string());
                }
            }
        }

        if in_source_section && install_cmd.is_none() {
            // Lines like `  id: pkg:npm/bash-language-server`
            if let Some(rest) = trimmed.strip_prefix("id:") {
                let purl = rest.trim();
                install_cmd = parse_purl_install_cmd(purl);
            }
        }
    }

    MasonPackageInfo {
        binaries,
        install_cmd,
        categories,
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
    /// Capabilities advertised by the server in the initialize response.
    pub capabilities: serde_json::Value,
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
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
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
            capabilities: serde_json::Value::Null,
        };

        // Send initialize request
        let root_uri = path_to_uri(root_path);
        let init_params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "workspace": {
                    "workspaceEdit": {
                        "documentChanges": true,
                        "normalizationMode": "None"
                    },
                    "applyEdit": true
                },
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
                    "rename": {
                        "dynamicRegistration": false,
                        "prepareSupport": false,
                        "honorsChangeAnnotations": false
                    },
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

    /// Request all references at a position.
    pub fn request_references(&mut self, uri: &str, line: u32, character: u32) -> i64 {
        self.send_request(
            "textDocument/references",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": true }
            }),
        )
    }

    /// Request go-to-implementation at a position.
    pub fn request_implementation(&mut self, uri: &str, line: u32, character: u32) -> i64 {
        self.send_request(
            "textDocument/implementation",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
    }

    /// Request go-to-type-definition at a position.
    pub fn request_type_definition(&mut self, uri: &str, line: u32, character: u32) -> i64 {
        self.send_request(
            "textDocument/typeDefinition",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
    }

    /// Request signature help at a position.
    pub fn request_signature_help(&mut self, uri: &str, line: u32, character: u32) -> i64 {
        self.send_request(
            "textDocument/signatureHelp",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
    }

    /// Whether the server advertises document formatting support.
    #[allow(dead_code)]
    pub fn supports_formatting(&self) -> bool {
        let v = &self.capabilities["documentFormattingProvider"];
        // Can be `true` or an options object (both mean supported).
        v.as_bool().unwrap_or(false) || v.is_object()
    }

    /// Request whole-file formatting.
    pub fn request_formatting(&mut self, uri: &str, tab_size: u32, insert_spaces: bool) -> i64 {
        self.send_request(
            "textDocument/formatting",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "options": {
                    "tabSize": tab_size,
                    "insertSpaces": insert_spaces
                }
            }),
        )
    }

    /// Request range formatting (available for visual-selection formatting).
    #[allow(dead_code)]
    pub fn request_range_formatting(
        &mut self,
        uri: &str,
        range: &LspRange,
        tab_size: u32,
        insert_spaces: bool,
    ) -> i64 {
        self.send_request(
            "textDocument/rangeFormatting",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": range.start.line, "character": range.start.character },
                    "end":   { "line": range.end.line,   "character": range.end.character }
                },
                "options": {
                    "tabSize": tab_size,
                    "insertSpaces": insert_spaces
                }
            }),
        )
    }

    /// Request rename of the symbol at a position.
    pub fn request_rename(&mut self, uri: &str, line: u32, character: u32, new_name: &str) -> i64 {
        self.send_request(
            "textDocument/rename",
            serde_json::json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "newName": new_name
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
                            if let Some(caps) = r.get("capabilities") {
                                initialized_sent = true;
                                let _ = tx.send(LspEvent::Initialized(server_id, caps.clone()));
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
                Some("textDocument/references") => {
                    let locations = result
                        .and_then(parse_locations_response)
                        .unwrap_or_default();
                    let _ = tx.send(LspEvent::ReferencesResponse {
                        server_id,
                        request_id: id,
                        locations,
                    });
                }
                Some("textDocument/implementation") => {
                    let locations = result
                        .and_then(parse_locations_response)
                        .unwrap_or_default();
                    let _ = tx.send(LspEvent::ImplementationResponse {
                        server_id,
                        request_id: id,
                        locations,
                    });
                }
                Some("textDocument/typeDefinition") => {
                    let locations = result
                        .and_then(parse_locations_response)
                        .unwrap_or_default();
                    let _ = tx.send(LspEvent::TypeDefinitionResponse {
                        server_id,
                        request_id: id,
                        locations,
                    });
                }
                Some("textDocument/signatureHelp") => {
                    if let Some(r) = result {
                        if let Some(event) = try_parse_signature_help_response(server_id, id, r) {
                            let _ = tx.send(event);
                        }
                    }
                }
                Some("textDocument/formatting") | Some("textDocument/rangeFormatting") => {
                    let edits = result.and_then(parse_text_edits).unwrap_or_default();
                    let _ = tx.send(LspEvent::FormattingResponse {
                        server_id,
                        request_id: id,
                        edits,
                    });
                }
                Some("textDocument/rename") => {
                    let null = serde_json::Value::Null;
                    let r = result.unwrap_or(&null);
                    let error_message = if result.is_none() {
                        json.get("error")
                            .and_then(|e| e.get("message"))
                            .and_then(|m| m.as_str())
                            .map(|s| s.to_string())
                    } else {
                        None
                    };
                    let workspace_edit = try_parse_workspace_edit(r);
                    let _ = tx.send(LspEvent::RenameResponse {
                        server_id,
                        request_id: id,
                        workspace_edit,
                        error_message,
                    });
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

/// Parse a response that is a flat array of Locations (references, implementation, typeDefinition).
fn parse_locations_response(result: &serde_json::Value) -> Option<Vec<Location>> {
    let mut locations = Vec::new();
    if result.is_null() {
        return Some(locations);
    }
    if let Some(arr) = result.as_array() {
        for loc in arr {
            if let Some(l) = parse_location(loc) {
                locations.push(l);
            } else if let Some(l) = parse_location_link(loc) {
                locations.push(l);
            }
        }
    } else if result.is_object() {
        if let Some(l) = parse_location(result) {
            locations.push(l);
        } else if let Some(l) = parse_location_link(result) {
            locations.push(l);
        }
    }
    Some(locations)
}

fn try_parse_signature_help_response(
    server_id: LspServerId,
    request_id: i64,
    result: &serde_json::Value,
) -> Option<LspEvent> {
    if result.is_null() {
        return None;
    }
    let signatures = result.get("signatures")?.as_array()?;
    let sig = signatures.first()?;
    let label = sig.get("label")?.as_str()?.to_string();

    // Parse parameter ranges — each can be [start, end] (UTF-16 offsets) or { label: str }
    let params: Vec<(usize, usize)> = sig
        .get("parameters")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let param_label = p.get("label")?;
                    if let Some(arr) = param_label.as_array() {
                        // [startOffset, endOffset] in bytes within label
                        let s = arr.first()?.as_u64()? as usize;
                        let e = arr.get(1)?.as_u64()? as usize;
                        Some((s, e))
                    } else if let Some(s) = param_label.as_str() {
                        // Find this substring in label
                        label.find(s).map(|start| (start, start + s.len()))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let active_param = result
        .get("activeParameter")
        .or_else(|| sig.get("activeParameter"))
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    Some(LspEvent::SignatureHelpResponse {
        server_id,
        request_id,
        label,
        params,
        active_param,
    })
}

/// Parse an array of TextEdit objects from a formatting response.
fn parse_text_edits(result: &serde_json::Value) -> Option<Vec<FormattingEdit>> {
    if result.is_null() {
        return Some(Vec::new());
    }
    let arr = result.as_array()?;
    let edits = arr
        .iter()
        .filter_map(|e| {
            let range = parse_range(e.get("range")?)?;
            let new_text = e.get("newText")?.as_str()?.to_string();
            Some(FormattingEdit { range, new_text })
        })
        .collect();
    Some(edits)
}

/// Parse a WorkspaceEdit from a rename response.
fn try_parse_workspace_edit(result: &serde_json::Value) -> WorkspaceEdit {
    let mut file_edits: Vec<FileEdit> = Vec::new();

    // Format 1: result.changes = { uri: [TextEdit] }
    if let Some(changes) = result.get("changes").and_then(|c| c.as_object()) {
        for (uri, edits_val) in changes {
            if let Some(path) = uri_to_path(uri) {
                if let Some(edits) = parse_text_edits(edits_val) {
                    file_edits.push(FileEdit { path, edits });
                }
            }
        }
    }

    // Format 2: result.documentChanges = [{ textDocument: { uri }, edits: [...] }]
    // Some entries may be file-level operations (kind: "create"/"rename"/"delete") with no
    // textDocument field — skip those rather than bailing out of the whole function.
    if let Some(doc_changes) = result.get("documentChanges").and_then(|d| d.as_array()) {
        for change in doc_changes {
            let Some(uri) = change
                .get("textDocument")
                .and_then(|td| td.get("uri"))
                .and_then(|u| u.as_str())
            else {
                continue;
            };
            if let Some(path) = uri_to_path(uri) {
                if let Some(edits_val) = change.get("edits") {
                    if let Some(edits) = parse_text_edits(edits_val) {
                        file_edits.push(FileEdit { path, edits });
                    }
                }
            }
        }
    }

    WorkspaceEdit {
        changes: file_edits,
    }
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

    // -----------------------------------------------------------------------
    // Mason PURL parser tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_purl_npm() {
        assert_eq!(
            parse_purl_install_cmd("pkg:npm/bash-language-server"),
            Some("npm install -g bash-language-server".to_string())
        );
    }

    #[test]
    fn test_parse_purl_npm_versioned() {
        // Version suffix should be stripped
        assert_eq!(
            parse_purl_install_cmd("pkg:npm/typescript-language-server@4.3.3"),
            Some("npm install -g typescript-language-server".to_string())
        );
    }

    #[test]
    fn test_parse_purl_nuget() {
        assert_eq!(
            parse_purl_install_cmd("pkg:nuget/csharp-ls"),
            Some("dotnet tool install -g csharp-ls".to_string())
        );
    }

    #[test]
    fn test_parse_purl_golang() {
        assert_eq!(
            parse_purl_install_cmd("pkg:golang/golang.org/x/tools/gopls"),
            Some("go install golang.org/x/tools/gopls@latest".to_string())
        );
    }

    #[test]
    fn test_parse_purl_pypi() {
        assert_eq!(
            parse_purl_install_cmd("pkg:pypi/python-lsp-server"),
            Some("pip install python-lsp-server".to_string())
        );
    }

    #[test]
    fn test_parse_purl_cargo() {
        assert_eq!(
            parse_purl_install_cmd("pkg:cargo/taplo-cli"),
            Some("cargo install taplo-cli".to_string())
        );
    }

    #[test]
    fn test_parse_purl_github_none() {
        // pkg:github has no automated install
        assert_eq!(
            parse_purl_install_cmd("pkg:github/sumneko/lua-language-server"),
            None
        );
    }

    #[test]
    fn test_parse_purl_generic_none() {
        assert_eq!(parse_purl_install_cmd("pkg:generic/omnisharp"), None);
    }

    // -----------------------------------------------------------------------
    // Mason package.yaml parser tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_mason_yaml_npm() {
        let yaml = r#"
name: bash-language-server
description: A language server for Bash
homepage: https://github.com/bash-lsp/bash-language-server
licenses:
  - MIT
languages:
  - Bash
categories:
  - LSP
source:
  id: pkg:npm/bash-language-server@5.4.0
bin:
  bash-language-server: node_modules/.bin/bash-language-server
"#;
        let info = parse_mason_package_yaml(yaml);
        assert_eq!(info.binaries, vec!["bash-language-server"]);
        assert_eq!(
            info.install_cmd,
            Some("npm install -g bash-language-server".to_string())
        );
        assert_eq!(info.categories, vec!["LSP"]);
    }

    #[test]
    fn test_parse_mason_yaml_no_bin() {
        // Package without a bin section (e.g. jdtls uses a wrapper script)
        let yaml = r#"
name: jdtls
source:
  id: pkg:generic/eclipse-jdt-ls
"#;
        let info = parse_mason_package_yaml(yaml);
        assert!(info.binaries.is_empty());
        assert_eq!(info.install_cmd, None); // pkg:generic has no install cmd
    }

    #[test]
    fn test_parse_mason_yaml_multiple_bins() {
        let yaml = r#"
name: typescript-language-server
source:
  id: pkg:npm/typescript-language-server@4.3.3
bin:
  typescript-language-server: node_modules/.bin/typescript-language-server
  tsserver: node_modules/.bin/tsserver
"#;
        let info = parse_mason_package_yaml(yaml);
        assert!(info
            .binaries
            .contains(&"typescript-language-server".to_string()));
        assert!(info.binaries.contains(&"tsserver".to_string()));
        assert_eq!(
            info.install_cmd,
            Some("npm install -g typescript-language-server".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // language_id_from_path new extensions
    // -----------------------------------------------------------------------

    #[test]
    fn test_language_id_csharp() {
        use std::path::Path;
        assert_eq!(
            language_id_from_path(Path::new("Foo.cs")),
            Some("csharp".to_string())
        );
    }

    #[test]
    fn test_language_id_kotlin() {
        use std::path::Path;
        assert_eq!(
            language_id_from_path(Path::new("Main.kt")),
            Some("kotlin".to_string())
        );
        assert_eq!(
            language_id_from_path(Path::new("build.gradle.kts")),
            Some("kotlin".to_string())
        );
    }

    #[test]
    fn test_language_id_dockerfile() {
        use std::path::Path;
        assert_eq!(
            language_id_from_path(Path::new("Dockerfile")),
            Some("dockerfile".to_string())
        );
        assert_eq!(
            language_id_from_path(Path::new("Dockerfile.prod")),
            Some("dockerfile".to_string())
        );
    }

    #[test]
    fn test_language_id_terraform() {
        use std::path::Path;
        assert_eq!(
            language_id_from_path(Path::new("main.tf")),
            Some("terraform".to_string())
        );
    }

    #[test]
    fn test_mason_package_for_language() {
        assert_eq!(
            mason_package_for_language("csharp"),
            Some("csharp-language-server")
        );
        assert_eq!(
            mason_package_for_language("lua"),
            Some("lua-language-server")
        );
        assert_eq!(mason_package_for_language("rust"), None); // handled by built-in registry
        assert_eq!(mason_package_for_language("scala"), None); // PATH-only
    }

    // -----------------------------------------------------------------------
    // Mason category parsing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_mason_yaml_dap_category() {
        let yaml = r#"
name: codelldb
description: LLDB-based debug adapter
categories:
  - DAP
source:
  id: pkg:github/vadimcn/codelldb
bin:
  codelldb: extension/adapter/codelldb
"#;
        let info = parse_mason_package_yaml(yaml);
        assert_eq!(info.categories, vec!["DAP"]);
        assert!(info.is_dap());
        assert!(!info.is_lsp());
        assert!(!info.is_linter());
    }

    #[test]
    fn test_parse_mason_yaml_linter_category() {
        let yaml = r#"
name: pylint
description: Python linter
categories:
  - Linter
source:
  id: pkg:pypi/pylint
bin:
  pylint: venv/bin/pylint
"#;
        let info = parse_mason_package_yaml(yaml);
        assert_eq!(info.categories, vec!["Linter"]);
        assert!(info.is_linter());
        assert!(!info.is_dap());
        assert!(!info.is_formatter());
    }

    #[test]
    fn test_parse_mason_yaml_multi_category() {
        let yaml = r#"
name: black
description: Python code formatter and linter
categories:
  - Linter
  - Formatter
source:
  id: pkg:pypi/black
bin:
  black: venv/bin/black
"#;
        let info = parse_mason_package_yaml(yaml);
        assert_eq!(info.categories, vec!["Linter", "Formatter"]);
        assert!(info.is_linter());
        assert!(info.is_formatter());
        assert!(!info.is_dap());
        assert!(!info.is_lsp());
    }

    #[test]
    fn test_parse_mason_yaml_lsp_still_works() {
        // Regression: adding categories field must not break existing LSP yaml parsing
        let yaml = r#"
name: rust-analyzer
description: Rust language server
categories:
  - LSP
source:
  id: pkg:cargo/rust-analyzer
bin:
  rust-analyzer: rust-analyzer
"#;
        let info = parse_mason_package_yaml(yaml);
        assert_eq!(info.binaries, vec!["rust-analyzer"]);
        assert_eq!(
            info.install_cmd,
            Some("cargo install rust-analyzer".to_string())
        );
        assert_eq!(info.categories, vec!["LSP"]);
        assert!(info.is_lsp());
        assert!(!info.is_dap());
    }
}
