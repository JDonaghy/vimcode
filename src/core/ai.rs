//! AI provider integration — send chat messages via curl subprocess.
//! Supports Anthropic (Claude), OpenAI-compatible APIs, and Ollama (local).

/// A single message in an AI conversation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiMessage {
    pub role: String, // "user" or "assistant"
    pub content: String,
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

    let output = std::process::Command::new("curl")
        .args([
            "-sf",
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
        ])
        .output()
        .map_err(|e| format!("curl error: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("curl failed: {err}"));
    }

    let resp: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;

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
        .ok_or_else(|| {
            format!(
                "unexpected response: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
}

// ── OpenAI-compatible ─────────────────────────────────────────────────────────

fn send_openai(
    api_key: &str,
    base_url: &str,
    model: &str,
    messages: &[AiMessage],
) -> Result<String, String> {
    let url = if base_url.is_empty() {
        "https://api.openai.com/v1/chat/completions".to_string()
    } else {
        format!("{}/v1/chat/completions", base_url.trim_end_matches('/'))
    };
    let model = if model.is_empty() { "gpt-4o" } else { model };
    let msgs_json = messages_to_json(messages);
    let body = format!(r#"{{"model":"{model}","messages":{msgs_json}}}"#);

    let output = std::process::Command::new("curl")
        .args([
            "-sf",
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
        ])
        .output()
        .map_err(|e| format!("curl error: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("curl failed: {err}"));
    }

    let resp: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;

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
        .ok_or_else(|| {
            format!(
                "unexpected response: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
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

    let output = std::process::Command::new("curl")
        .args([
            "-sf",
            "--max-time",
            "120",
            "-X",
            "POST",
            &url,
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
        ])
        .output()
        .map_err(|e| format!("curl error: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("curl failed: {err}"));
    }

    let resp: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;

    resp.pointer("/message/content")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            format!(
                "unexpected response: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        })
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

#[cfg(test)]
mod tests {
    use super::*;

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
