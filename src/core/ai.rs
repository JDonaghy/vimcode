//! AI provider integration — send chat messages via curl subprocess.
//! Supports Anthropic (Claude), OpenAI-compatible APIs, and Ollama (local).

/// A single message in an AI conversation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiMessage {
    /// "user" or "assistant" for the direct-provider (curl) transport.
    ///
    /// The ACP transport (#952, `Engine::poll_acp` in
    /// `src/core/engine/acp_ops.rs`) also pushes `"assistant-thought"` —
    /// reused for two different things that happen to render the same way
    /// (quadraui's "System" role label): genuine `agent_thought_chunk`
    /// reasoning text, *and* unrelated system/error notices (agent failed
    /// to start, a request failed, a turn stopped early). That conflation
    /// satisfies ACP-1's "thoughts are visually distinct from messages"
    /// bar but is a minor design smell — a follow-up slice may want a
    /// distinct role (e.g. `"system"`) for the notice case so it isn't
    /// visually indistinguishable from real agent reasoning.
    ///
    /// Only `"user"`/`"assistant"` are ever sent to a direct-provider API
    /// (`ai_send_message_via_curl` filters the rest) — ACP-only roles like
    /// `"assistant-thought"` aren't recognised request shapes for
    /// Anthropic/OpenAI/Ollama.
    pub role: String,
    pub content: String,
}

/// Request an inline code completion (ghost text) from an AI provider.
///
/// Runs synchronously (blocking curl); call from a background thread.
///
/// `prefix` is the text before the cursor; `suffix` is text after the cursor
/// (may be empty).  Returns the text that should be inserted at the cursor.
pub fn complete(
    provider: &str,
    api_key: &str,
    base_url: &str,
    model: &str,
    prefix: &str,
    suffix: &str,
) -> Result<String, String> {
    // Build a fill-in-the-middle prompt. The AI should return only the
    // code/text that belongs between `prefix` and `suffix`, with no
    // explanation or markdown fencing.
    let system = "You are a code completion engine. \
        Output ONLY the text that should be inserted at the cursor — \
        no explanation, no markdown, no backticks. \
        Keep completions concise (usually one line, occasionally a short block).";

    let user_msg = if suffix.is_empty() {
        format!("Complete the following code at the cursor position marked with <CURSOR>:\n\n{prefix}<CURSOR>")
    } else {
        format!("Complete the following code at the cursor position marked with <CURSOR>:\n\n{prefix}<CURSOR>{suffix}")
    };

    let messages = [AiMessage {
        role: "user".to_string(),
        content: user_msg,
    }];

    send_chat(provider, api_key, base_url, model, &messages, system)
}

/// Whether `provider` needs an API key at all. Ollama is local/no-auth;
/// Anthropic and OpenAI (and OpenAI-compatible providers routed through
/// `send_openai`) require one.
pub fn provider_needs_api_key(provider: &str) -> bool {
    provider != "ollama"
}

/// Resolve the effective API key for `provider`: the provider's environment
/// variable (`ANTHROPIC_API_KEY` / `OPENAI_API_KEY`) takes priority over
/// `setting_key`, matching `send_anthropic`/`send_openai`'s own resolution
/// order below. Ollama never needs a key, so this returns empty for it.
pub fn resolve_api_key(provider: &str, setting_key: &str) -> String {
    let env_var = match provider {
        "openai" => "OPENAI_API_KEY",
        "ollama" => return String::new(),
        _ => "ANTHROPIC_API_KEY",
    };
    let env_val = std::env::var(env_var).unwrap_or_default();
    if !env_val.is_empty() {
        env_val
    } else {
        setting_key.to_string()
    }
}

/// Send a chat request to an AI provider and return the assistant's reply.
///
/// Runs synchronously (blocking curl); call from a background thread.
///
/// - `provider`:  `"anthropic"`, `"openai"`, or `"ollama"`
/// - `api_key`:   API key (empty string for Ollama)
/// - `base_url`:  Override base URL; empty = provider default
/// - `model`:     Model name; empty = sensible provider default
/// - `messages`:  Conversation history (user + assistant turns)
/// - `system`:    Optional system prompt inserted before the conversation
pub fn send_chat(
    provider: &str,
    api_key: &str,
    base_url: &str,
    model: &str,
    messages: &[AiMessage],
    system: &str,
) -> Result<String, String> {
    match provider {
        "openai" => send_openai(api_key, base_url, model, messages),
        "ollama" => send_ollama(base_url, model, messages, system),
        _ => send_anthropic(api_key, base_url, model, messages, system),
    }
}

// ── Anthropic ─────────────────────────────────────────────────────────────────

fn send_anthropic(
    api_key: &str,
    base_url: &str,
    model: &str,
    messages: &[AiMessage],
    system: &str,
) -> Result<String, String> {
    // Env var takes priority; settings value is a fallback.
    let resolved_key = resolve_api_key("anthropic", api_key);
    let api_key = &resolved_key;

    let url = if base_url.is_empty() {
        "https://api.anthropic.com/v1/messages".to_string()
    } else {
        format!("{}/v1/messages", base_url.trim_end_matches('/'))
    };
    let model = if model.is_empty() {
        "claude-sonnet-4-6"
    } else {
        model
    };

    // Build JSON body
    let msgs_json = messages_to_json(messages);
    let system_fragment = if system.is_empty() {
        String::new()
    } else {
        let escaped = escape_json_string(system);
        format!(r#","system":"{escaped}""#)
    };
    let body = format!(
        r#"{{"model":"{model}","max_tokens":4096,"messages":{msgs_json}{system_fragment}}}"#
    );

    let output = crate::core::git::hidden_command("curl")
        .args([
            "-s",
            "--max-time",
            "120",
            "-X",
            "POST",
            &url,
            "-H",
            "Content-Type: application/json",
            "-H",
            &format!("x-api-key: {api_key}"),
            "-H",
            "anthropic-version: 2023-06-01",
            "-d",
            &body,
            "-w",
            HTTP_STATUS_SUFFIX,
        ])
        .output()
        .map_err(|e| format!("curl error: {e}"))?;

    if !output.status.success() {
        return Err(curl_process_failure_message(
            output.status.code(),
            &String::from_utf8_lossy(&output.stderr),
        ));
    }

    let (resp_body, status) = split_curl_output(&output.stdout);
    if !(200..300).contains(&status) {
        return Err(format!(
            "curl request failed: HTTP {status} — {}",
            trim_body_for_error(&resp_body)
        ));
    }

    let resp: serde_json::Value = serde_json::from_str(&resp_body).map_err(|e| e.to_string())?;

    // API-level error
    if resp.get("type").and_then(|t| t.as_str()) == Some("error") {
        let msg = resp
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown API error");
        return Err(msg.to_string());
    }

    resp.pointer("/content/0/text")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("unexpected response: {resp_body}"))
}

// ── OpenAI-compatible ─────────────────────────────────────────────────────────

fn send_openai(
    api_key: &str,
    base_url: &str,
    model: &str,
    messages: &[AiMessage],
) -> Result<String, String> {
    let resolved_key = resolve_api_key("openai", api_key);
    let api_key = &resolved_key;

    let url = if base_url.is_empty() {
        "https://api.openai.com/v1/chat/completions".to_string()
    } else {
        format!("{}/v1/chat/completions", base_url.trim_end_matches('/'))
    };
    let model = if model.is_empty() { "gpt-4o" } else { model };
    let msgs_json = messages_to_json(messages);
    let body = format!(r#"{{"model":"{model}","messages":{msgs_json}}}"#);

    let output = crate::core::git::hidden_command("curl")
        .args([
            "-s",
            "--max-time",
            "120",
            "-X",
            "POST",
            &url,
            "-H",
            "Content-Type: application/json",
            "-H",
            &format!("Authorization: Bearer {api_key}"),
            "-d",
            &body,
            "-w",
            HTTP_STATUS_SUFFIX,
        ])
        .output()
        .map_err(|e| format!("curl error: {e}"))?;

    if !output.status.success() {
        return Err(curl_process_failure_message(
            output.status.code(),
            &String::from_utf8_lossy(&output.stderr),
        ));
    }

    let (resp_body, status) = split_curl_output(&output.stdout);
    if !(200..300).contains(&status) {
        return Err(format!(
            "curl request failed: HTTP {status} — {}",
            trim_body_for_error(&resp_body)
        ));
    }

    let resp: serde_json::Value = serde_json::from_str(&resp_body).map_err(|e| e.to_string())?;

    if let Some(err_obj) = resp.get("error") {
        let msg = err_obj
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown API error");
        return Err(msg.to_string());
    }

    resp.pointer("/choices/0/message/content")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("unexpected response: {resp_body}"))
}

// ── Ollama ────────────────────────────────────────────────────────────────────

fn send_ollama(
    base_url: &str,
    model: &str,
    messages: &[AiMessage],
    system: &str,
) -> Result<String, String> {
    let base = if base_url.is_empty() {
        "http://localhost:11434"
    } else {
        base_url.trim_end_matches('/')
    };
    let url = format!("{base}/api/chat");
    let model = if model.is_empty() { "llama3.2" } else { model };

    // Prepend system message if provided
    let all_messages: Vec<serde_json::Value> = if system.is_empty() {
        messages
            .iter()
            .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
            .collect()
    } else {
        let mut v = vec![serde_json::json!({"role": "system", "content": system})];
        v.extend(
            messages
                .iter()
                .map(|m| serde_json::json!({"role": m.role, "content": m.content})),
        );
        v
    };
    let msgs_str = serde_json::to_string(&all_messages).map_err(|e| e.to_string())?;
    let body = format!(r#"{{"model":"{model}","stream":false,"messages":{msgs_str}}}"#);

    let output = crate::core::git::hidden_command("curl")
        .args([
            "-s",
            "--max-time",
            "120",
            "-X",
            "POST",
            &url,
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
            "-w",
            HTTP_STATUS_SUFFIX,
        ])
        .output()
        .map_err(|e| format!("curl error: {e}"))?;

    if !output.status.success() {
        return Err(curl_process_failure_message(
            output.status.code(),
            &String::from_utf8_lossy(&output.stderr),
        ));
    }

    let (resp_body, status) = split_curl_output(&output.stdout);
    if !(200..300).contains(&status) {
        return Err(format!(
            "curl request failed: HTTP {status} — {}",
            trim_body_for_error(&resp_body)
        ));
    }

    let resp: serde_json::Value = serde_json::from_str(&resp_body).map_err(|e| e.to_string())?;

    resp.pointer("/message/content")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("unexpected response: {resp_body}"))
}

// ── curl error reporting (#1446) ────────────────────────────────────────────────
//
// `-sf` (silent + fail-on-error) was the original flag combo: `-s` suppresses
// curl's own progress/diagnostic output and `-f` makes curl exit nonzero
// *and discard the response body* on a non-2xx HTTP status. Combined, a
// failed request produced an empty stderr AND an empty stdout — the only
// signal left was curl's exit code, which the old code didn't even read.
// That's how `:AI hello` with no key configured produced the bare
// "AI error: curl failed:" this issue is about.
//
// The fix drops `-f` and instead appends the HTTP status code to stdout via
// `-w` so a non-2xx response can be reported with its real status and body
// instead of being swallowed.

/// `-w` write-out format appended to every curl invocation in this module:
/// a newline followed by the HTTP status code, so a non-2xx response can be
/// distinguished from success without relying on curl's own `-f` exit code
/// (which discards the body).
const HTTP_STATUS_SUFFIX: &str = "\n%{http_code}";

/// Split curl's stdout (the response body followed by the `HTTP_STATUS_SUFFIX`
/// write-out) into `(body, status_code)`. Returns status `0` if the trailing
/// status line is missing or unparsable (should not happen given
/// `HTTP_STATUS_SUFFIX` is always passed, but keeps this infallible).
fn split_curl_output(stdout: &[u8]) -> (String, u16) {
    let text = String::from_utf8_lossy(stdout);
    match text.rsplit_once('\n') {
        Some((body, status)) => (body.to_string(), status.trim().parse().unwrap_or(0)),
        None => (String::new(), 0),
    }
}

/// Trim a response body to a reasonable size for an inline error message.
fn trim_body_for_error(body: &str) -> String {
    const MAX_CHARS: usize = 500;
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "(empty response body)".to_string();
    }
    if trimmed.chars().count() > MAX_CHARS {
        let prefix: String = trimmed.chars().take(MAX_CHARS).collect();
        format!("{prefix}...")
    } else {
        trimmed.to_string()
    }
}

/// Build a message for a curl *process*-level failure (nonzero exit before
/// any HTTP response was even received — DNS failure, connection refused,
/// TLS error, timeout, curl missing, etc). With `-s`, curl's own stderr is
/// almost always empty, so the exit code is included unconditionally rather
/// than being the thing that's silently dropped (#1446).
fn curl_process_failure_message(exit_code: Option<i32>, stderr: &str) -> String {
    let code = exit_code
        .map(|c| c.to_string())
        .unwrap_or_else(|| "unknown (terminated by signal)".to_string());
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!(
            "curl failed with exit code {code} (no output from curl — check network \
             connectivity and that curl is installed)"
        )
    } else {
        format!("curl failed with exit code {code}: {stderr}")
    }
}

// ── JSON helpers ──────────────────────────────────────────────────────────────

/// Serialize a slice of messages to a JSON array string without serde_json allocation.
fn messages_to_json(messages: &[AiMessage]) -> String {
    let items: Vec<String> = messages
        .iter()
        .map(|m| {
            let role = escape_json_string(&m.role);
            let content = escape_json_string(&m.content);
            format!(r#"{{"role":"{role}","content":"{content}"}}"#)
        })
        .collect();
    format!("[{}]", items.join(","))
}

/// Escape a string for safe embedding inside a JSON string literal.
fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str(r#"\""#),
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            c if (c as u32) < 0x20 => {
                // ASCII control characters
                let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

/// Serializes every test *anywhere in this crate* that mutates the
/// process-global `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` env vars (read by
/// [`resolve_api_key`]) against every other one — `std::env::set_var` is
/// process-global and Rust's default test runner executes `#[test]`s in
/// parallel threads within one process, so this module's own env-var tests
/// and `engine::tests`' #1446 coverage (which needs these vars reliably
/// unset to exercise the "no API key configured" path) would otherwise race
/// each other. See `crate::core::paths::VIMCODE_TEST_DATA_HOME_LOCK`'s doc
/// comment for the same tradeoff applied to a different env var.
#[cfg(test)]
pub(crate) static AI_API_KEY_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// RAII guard: snapshot + restore an environment variable across a test.
#[cfg(test)]
pub(crate) struct EnvVarGuard {
    key: &'static str,
    old: Option<String>,
}

#[cfg(test)]
impl EnvVarGuard {
    pub(crate) fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, old }
    }

    pub(crate) fn unset(key: &'static str) -> Self {
        let old = std::env::var(key).ok();
        std::env::remove_var(key);
        Self { key, old }
    }
}

#[cfg(test)]
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.old.take() {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_needs_api_key() {
        assert!(provider_needs_api_key("anthropic"));
        assert!(provider_needs_api_key("openai"));
        assert!(!provider_needs_api_key("ollama"));
    }

    #[test]
    fn test_resolve_api_key_ollama_always_empty() {
        assert_eq!(resolve_api_key("ollama", "some-setting-key"), "");
    }

    #[test]
    fn test_resolve_api_key_falls_back_to_setting_when_env_unset() {
        let _lock = AI_API_KEY_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = EnvVarGuard::unset("ANTHROPIC_API_KEY");
        assert_eq!(resolve_api_key("anthropic", "setting-key"), "setting-key");
    }

    #[test]
    fn test_resolve_api_key_env_var_takes_priority_over_setting() {
        let _lock = AI_API_KEY_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = EnvVarGuard::set("ANTHROPIC_API_KEY", "env-key");
        assert_eq!(resolve_api_key("anthropic", "setting-key"), "env-key");
    }

    #[test]
    fn test_resolve_api_key_openai_uses_openai_env_var() {
        let _lock = AI_API_KEY_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = EnvVarGuard::set("OPENAI_API_KEY", "env-openai-key");
        assert_eq!(resolve_api_key("openai", "setting-key"), "env-openai-key");
    }

    /// #1446 (2): a curl *process*-level failure (nonzero exit, e.g. DNS
    /// failure or connection refused) with `-s`'s empty stderr must still
    /// produce a non-empty, actionable message — not the bare "curl
    /// failed:" the bug reported. RED verified: reverting to
    /// `format!("curl failed: {err}")` with an empty `err` reproduces
    /// exactly that bare string.
    #[test]
    fn test_curl_process_failure_message_nonempty_with_empty_stderr() {
        let msg = curl_process_failure_message(Some(7), "");
        assert!(!msg.is_empty());
        assert!(
            msg.contains('7'),
            "message should mention the exit code: {msg}"
        );
    }

    #[test]
    fn test_curl_process_failure_message_includes_stderr_when_present() {
        let msg = curl_process_failure_message(Some(6), "Could not resolve host");
        assert!(msg.contains('6'));
        assert!(msg.contains("Could not resolve host"));
    }

    #[test]
    fn test_curl_process_failure_message_handles_missing_exit_code() {
        let msg = curl_process_failure_message(None, "");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_split_curl_output_separates_body_and_status() {
        let stdout = b"{\"ok\":true}\n200";
        let (body, status) = split_curl_output(stdout);
        assert_eq!(body, r#"{"ok":true}"#);
        assert_eq!(status, 200);
    }

    #[test]
    fn test_split_curl_output_http_error_status() {
        let stdout = b"{\"error\":\"unauthorized\"}\n401";
        let (body, status) = split_curl_output(stdout);
        assert_eq!(body, r#"{"error":"unauthorized"}"#);
        assert_eq!(status, 401);
    }

    #[test]
    fn test_trim_body_for_error_empty_body() {
        assert_eq!(trim_body_for_error(""), "(empty response body)");
    }

    #[test]
    fn test_trim_body_for_error_truncates_long_body() {
        let long = "x".repeat(1000);
        let trimmed = trim_body_for_error(&long);
        assert!(trimmed.ends_with("..."));
        assert!(trimmed.len() < long.len());
    }

    #[test]
    fn test_escape_json_string_basic() {
        assert_eq!(escape_json_string("hello"), "hello");
        assert_eq!(escape_json_string(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(escape_json_string("line1\nline2"), r"line1\nline2");
        assert_eq!(escape_json_string("tab\there"), r"tab\there");
    }

    #[test]
    fn test_messages_to_json_empty() {
        assert_eq!(messages_to_json(&[]), "[]");
    }

    #[test]
    fn test_messages_to_json_single() {
        let msgs = vec![AiMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
        }];
        assert_eq!(
            messages_to_json(&msgs),
            r#"[{"role":"user","content":"hello"}]"#
        );
    }

    #[test]
    fn test_messages_to_json_escaping() {
        let msgs = vec![AiMessage {
            role: "user".to_string(),
            content: "say \"hello\"\nnew line".to_string(),
        }];
        let json = messages_to_json(&msgs);
        assert!(json.contains(r#"say \"hello\""#));
        assert!(json.contains(r"\n"));
    }

    #[test]
    fn test_ai_message_serde() {
        let msg = AiMessage {
            role: "assistant".to_string(),
            content: "Hello!".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: AiMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.role, "assistant");
        assert_eq!(back.content, "Hello!");
    }
}
