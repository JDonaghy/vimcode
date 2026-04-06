//! DAP protocol transport — new infrastructure module; fields used by poll_dap (Session 84+).
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Read as IoRead, Write as IoWrite};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// Events emitted by a DAP adapter; sent via channel to the engine.
#[derive(Debug)]
pub enum DapEvent {
    Initialized,
    Stopped {
        thread_id: u64,
        reason: String,
        hit_breakpoint_ids: Vec<u64>,
    },
    Continued {
        thread_id: u64,
    },
    Exited {
        exit_code: i64,
    },
    Output {
        category: String,
        output: String,
    },
    Breakpoint {
        reason: String,
        breakpoint: DapBreakpoint,
    },
    RequestComplete {
        seq: u64,
        command: String,
        success: bool,
        body: serde_json::Value,
        /// Human-readable error text from the `message` field of a DAP response.
        /// Populated by adapters like codelldb when a request fails.
        error_message: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct DapBreakpoint {
    pub id: Option<u64>,
    pub verified: bool,
    pub line: u64,
    pub source: String,
    /// Adapter-supplied reason string when the breakpoint is unverified.
    pub message: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub id: u64,
    pub name: String,
    pub source: Option<String>,
    pub line: u64,
}

/// A user-defined breakpoint with optional condition and hit count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakpointInfo {
    pub line: u64,
    /// Expression condition — adapter evaluates and only stops when truthy.
    pub condition: Option<String>,
    /// Hit-count condition — e.g. `">= 5"`, `"% 3"` (adapter-specific syntax).
    pub hit_condition: Option<String>,
    /// Log message — adapter prints this instead of stopping (logpoint).
    pub log_message: Option<String>,
}

impl BreakpointInfo {
    pub fn new(line: u64) -> Self {
        Self {
            line,
            condition: None,
            hit_condition: None,
            log_message: None,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DapVariable {
    pub name: String,
    pub value: String,
    pub var_ref: u64,
    /// True when `presentationHint.visibility` is "private", "protected", or "internal".
    pub is_nonpublic: bool,
}

// ---------------------------------------------------------------------------
// DapServer — manages a running debug adapter process
// ---------------------------------------------------------------------------

pub struct DapServer {
    stdin: BufWriter<Box<dyn IoWrite + Send>>,
    seq: u64,
    pub rx: mpsc::Receiver<DapEvent>,
    // Held only to keep the thread alive; dropped on DapServer drop.
    _thread: Option<JoinHandle<()>>,
    /// Raw JSON body of the last request sent (for diagnostic logging).
    pub last_sent_json: Option<String>,
    /// Maps outgoing request seq → command name.  Used to resolve the command
    /// when the adapter (codelldb) omits the `command` field in responses.
    pending_commands: HashMap<u64, String>,
}

impl DapServer {
    /// Launch a debug adapter process that communicates over stdin/stdout.
    pub fn spawn(cmd: &str, args: &[&str]) -> Result<Self, String> {
        let mut command = std::process::Command::new(cmd);
        command
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        // Start the adapter in a brand-new session (setsid) so it — and any
        // children it spawns (e.g. debugpy launching the target script) — are
        // fully detached from the editor's controlling terminal.  Without this,
        // the child can call tcsetpgrp() to steal the foreground process group,
        // which causes SIGTTIN and suspends the editor.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: setsid() is async-signal-safe (required by pre_exec).
            unsafe {
                command.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x00000200); // CREATE_NEW_PROCESS_GROUP
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Failed to spawn DAP adapter '{cmd}': {e}"))?;

        let stdin = child.stdin.take().ok_or("No stdin on DAP process")?;
        let stdout = child.stdout.take().ok_or("No stdout on DAP process")?;

        let (tx, rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            dap_reader_thread(stdout, tx);
            // Keep child alive until reader exits
            drop(child);
        });

        Ok(Self {
            stdin: BufWriter::new(Box::new(stdin)),
            seq: 1,
            rx,
            _thread: Some(thread),
            last_sent_json: None,
            pending_commands: HashMap::new(),
        })
    }

    /// Launch a debug adapter that communicates over TCP (e.g. codelldb).
    ///
    /// Strategy: find a free local port ourselves, spawn the adapter with
    /// `--port <N>` substituted into `args` (the placeholder `"0"` after
    /// `"--port"` is replaced), wait briefly for the adapter to start
    /// listening, then connect.  This avoids parsing adapter stdout (which
    /// may be buffered and never flushed to the pipe).
    pub fn spawn_tcp(cmd: &str, args: &[&str]) -> Result<Self, String> {
        // Pick a free port by binding and immediately releasing it.
        let port = {
            let tmp = std::net::TcpListener::bind("127.0.0.1:0")
                .map_err(|e| format!("Cannot find free port: {e}"))?;
            tmp.local_addr()
                .map_err(|e| format!("Cannot get bound port: {e}"))?
                .port()
        };
        let port_str = port.to_string();

        // Replace the placeholder "0" that follows "--port" or "--listen" in
        // args with the actual chosen port number.
        // codelldb uses "--port 0"; debugpy uses "--listen 0".
        let resolved_args: Vec<&str> = {
            let mut v: Vec<&str> = args.to_vec();
            let port_flag_pos = v
                .iter()
                .position(|a| *a == "--port")
                .or_else(|| v.iter().position(|a| *a == "--listen"));
            if let Some(pos) = port_flag_pos {
                if pos + 1 < v.len() {
                    v[pos + 1] = &port_str;
                }
            }
            v
        };

        let mut tcp_command = std::process::Command::new(cmd);
        tcp_command
            .args(&resolved_args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                tcp_command.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            tcp_command.creation_flags(0x00000200); // CREATE_NEW_PROCESS_GROUP
        }
        let child = tcp_command
            .spawn()
            .map_err(|e| format!("Failed to spawn DAP adapter '{cmd}': {e}"))?;

        // Give the adapter a moment to start listening.
        // 50 × 100 ms = 5 s — generous enough for debugpy (Python startup is slow).
        let tcp = {
            let mut last_err = String::new();
            let mut tcp = None;
            for _ in 0..50 {
                match std::net::TcpStream::connect(format!("127.0.0.1:{port}")) {
                    Ok(s) => {
                        tcp = Some(s);
                        break;
                    }
                    Err(e) => {
                        last_err = e.to_string();
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }
            tcp.ok_or_else(|| format!("Cannot connect to DAP adapter at port {port}: {last_err}"))?
        };

        let tcp_write = tcp
            .try_clone()
            .map_err(|e| format!("Cannot clone TCP stream: {e}"))?;

        let (tx, rx) = mpsc::channel();
        let thread = thread::spawn(move || {
            dap_reader_thread(tcp, tx);
            drop(child);
        });

        Ok(Self {
            stdin: BufWriter::new(Box::new(tcp_write)),
            seq: 1,
            rx,
            _thread: Some(thread),
            last_sent_json: None,
            pending_commands: HashMap::new(),
        })
    }

    /// Write a DAP request frame and return the seq number used.
    pub fn send_request(&mut self, command: &str, args: serde_json::Value) -> u64 {
        let seq = self.seq;
        self.seq += 1;
        self.pending_commands.insert(seq, command.to_string());
        let msg = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": args,
        });
        let body = msg.to_string();
        self.last_sent_json = Some(body.clone());
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let _ = self.stdin.write_all(header.as_bytes());
        let _ = self.stdin.write_all(body.as_bytes());
        let _ = self.stdin.flush();
        seq
    }

    /// Look up the command name for a given request seq and remove it from
    /// the pending map.  Returns the command or an empty string if unknown.
    pub fn resolve_command(&mut self, req_seq: u64) -> String {
        self.pending_commands.remove(&req_seq).unwrap_or_default()
    }

    /// Non-blocking drain of the event channel.
    pub fn poll(&mut self) -> Vec<DapEvent> {
        let mut events = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            events.push(ev);
        }
        events
    }

    // -- Standard request helpers --

    pub fn initialize(&mut self, adapter_id: &str) -> u64 {
        self.send_request(
            "initialize",
            serde_json::json!({
                "adapterID": adapter_id,
                "clientID": "vimcode",
                "clientName": "VimCode",
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "pathFormat": "path",
                "supportsVariableType": true,
                // Enables virtual variable groups in netcoredbg (e.g. "Non-Public Members")
                // and paged variable responses in other adapters.
                "supportsVariablePaging": true,
                "supportsRunInTerminalRequest": false,
            }),
        )
    }

    pub fn launch(&mut self, config: serde_json::Value) -> u64 {
        self.send_request("launch", config)
    }

    #[allow(dead_code)]
    pub fn attach(&mut self, config: serde_json::Value) -> u64 {
        self.send_request("attach", config)
    }

    pub fn set_breakpoints(&mut self, source: &str, bps: &[BreakpointInfo]) -> u64 {
        let bp_list: Vec<serde_json::Value> = bps
            .iter()
            .map(|bp| {
                let mut obj = serde_json::json!({ "line": bp.line });
                if let Some(cond) = &bp.condition {
                    obj["condition"] = serde_json::json!(cond);
                }
                if let Some(hc) = &bp.hit_condition {
                    obj["hitCondition"] = serde_json::json!(hc);
                }
                if let Some(msg) = &bp.log_message {
                    obj["logMessage"] = serde_json::json!(msg);
                }
                obj
            })
            .collect();
        // Include `name` (just the filename) alongside the full `path` so
        // adapters that use the short name for source lookup also work.
        let source_name = std::path::Path::new(source)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(source);
        self.send_request(
            "setBreakpoints",
            serde_json::json!({
                "source": { "path": source, "name": source_name },
                "breakpoints": bp_list,
            }),
        )
    }

    pub fn configuration_done(&mut self) -> u64 {
        self.send_request("configurationDone", serde_json::Value::Null)
    }

    pub fn continue_thread(&mut self, thread_id: u64) -> u64 {
        self.send_request("continue", serde_json::json!({ "threadId": thread_id }))
    }

    pub fn pause(&mut self, thread_id: u64) -> u64 {
        self.send_request("pause", serde_json::json!({ "threadId": thread_id }))
    }

    pub fn disconnect(&mut self) -> u64 {
        self.send_request("disconnect", serde_json::json!({ "restart": false }))
    }

    pub fn next(&mut self, thread_id: u64) -> u64 {
        self.send_request("next", serde_json::json!({ "threadId": thread_id }))
    }

    pub fn step_in(&mut self, thread_id: u64) -> u64 {
        self.send_request("stepIn", serde_json::json!({ "threadId": thread_id }))
    }

    pub fn step_out(&mut self, thread_id: u64) -> u64 {
        self.send_request("stepOut", serde_json::json!({ "threadId": thread_id }))
    }

    #[allow(dead_code)]
    pub fn stack_trace(&mut self, thread_id: u64) -> u64 {
        self.send_request("stackTrace", serde_json::json!({ "threadId": thread_id }))
    }

    #[allow(dead_code)]
    pub fn scopes(&mut self, frame_id: u64) -> u64 {
        self.send_request("scopes", serde_json::json!({ "frameId": frame_id }))
    }

    #[allow(dead_code)]
    pub fn variables(&mut self, var_ref: u64) -> u64 {
        self.send_request(
            "variables",
            serde_json::json!({ "variablesReference": var_ref }),
        )
    }

    /// Evaluate an expression in the context of a specific stack frame.
    /// `context` is typically `"repl"` for interactive evaluation.
    #[allow(dead_code)]
    pub fn evaluate(&mut self, expression: &str, frame_id: u64) -> u64 {
        self.send_request(
            "evaluate",
            serde_json::json!({
                "expression": expression,
                "frameId": frame_id,
                "context": "repl",
            }),
        )
    }
}

// ---------------------------------------------------------------------------
// Reader thread — parses Content-Length frames from adapter stdout
// ---------------------------------------------------------------------------

fn dap_reader_thread(stdout: impl IoRead + Send + 'static, tx: mpsc::Sender<DapEvent>) {
    let mut reader = BufReader::new(stdout);
    let mut header_buf = String::new();

    loop {
        // Read headers until blank line
        let mut content_length: Option<usize> = None;
        loop {
            header_buf.clear();
            match reader.read_line(&mut header_buf) {
                Ok(0) => return, // EOF — adapter exited
                Ok(_) => {
                    let trimmed = header_buf.trim();
                    if trimmed.is_empty() {
                        break; // End of headers
                    }
                    if let Some(val) = trimmed.strip_prefix("Content-Length:") {
                        if let Ok(len) = val.trim().parse::<usize>() {
                            content_length = Some(len);
                        }
                    }
                }
                Err(_) => return,
            }
        }

        let content_length = match content_length {
            Some(len) => len,
            None => continue, // Malformed message — skip
        };

        // Read body
        let mut body = vec![0u8; content_length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }

        let body_str = match std::str::from_utf8(&body) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let msg: serde_json::Value = match serde_json::from_str(body_str) {
            Ok(v) => v,
            Err(_) => continue,
        };

        match msg.get("type").and_then(|t| t.as_str()) {
            Some("event") => {
                if let Some(ev) = parse_dap_event(&msg) {
                    if tx.send(ev).is_err() {
                        return;
                    }
                }
            }
            Some("response") => {
                let seq = msg.get("request_seq").and_then(|v| v.as_u64()).unwrap_or(0);
                let command = msg
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let success = msg
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let body = msg.get("body").cloned().unwrap_or(serde_json::Value::Null);
                let error_message = msg
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                if tx
                    .send(DapEvent::RequestComplete {
                        seq,
                        command,
                        success,
                        body,
                        error_message,
                    })
                    .is_err()
                {
                    return;
                }
            }
            _ => {}
        }
    }
}

fn parse_dap_event(msg: &serde_json::Value) -> Option<DapEvent> {
    let event_name = msg.get("event")?.as_str()?;
    let body = msg.get("body");

    match event_name {
        "initialized" => Some(DapEvent::Initialized),
        "stopped" => {
            let b = body?;
            let thread_id = b.get("threadId").and_then(|v| v.as_u64()).unwrap_or(0);
            let reason = b
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("paused")
                .to_string();
            let hit_breakpoint_ids = b
                .get("hitBreakpointIds")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.as_u64()).collect())
                .unwrap_or_default();
            Some(DapEvent::Stopped {
                thread_id,
                reason,
                hit_breakpoint_ids,
            })
        }
        "continued" => {
            let thread_id = body
                .and_then(|b| b.get("threadId"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            Some(DapEvent::Continued { thread_id })
        }
        "exited" | "terminated" => {
            let exit_code = body
                .and_then(|b| b.get("exitCode"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            Some(DapEvent::Exited { exit_code })
        }
        "output" => {
            let b = body?;
            let category = b
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("console")
                .to_string();
            let output = b
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(DapEvent::Output { category, output })
        }
        "breakpoint" => {
            let b = body?;
            let reason = b
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("changed")
                .to_string();
            let bp = b.get("breakpoint")?;
            let breakpoint = DapBreakpoint {
                id: bp.get("id").and_then(|v| v.as_u64()),
                verified: bp
                    .get("verified")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                line: bp.get("line").and_then(|v| v.as_u64()).unwrap_or(0),
                source: bp
                    .get("source")
                    .and_then(|s| s.get("path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                message: bp
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            };
            Some(DapEvent::Breakpoint { reason, breakpoint })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dap_breakpoint_fields() {
        let bp = DapBreakpoint {
            id: Some(7),
            verified: true,
            line: 42,
            source: "/src/main.rs".to_string(),
            message: None,
        };
        assert_eq!(bp.id, Some(7));
        assert!(bp.verified);
        assert_eq!(bp.line, 42);
        assert_eq!(bp.source, "/src/main.rs");
    }

    #[test]
    fn test_parse_dap_event_initialized() {
        let msg = serde_json::json!({ "type": "event", "event": "initialized" });
        assert!(matches!(parse_dap_event(&msg), Some(DapEvent::Initialized)));
    }

    #[test]
    fn test_parse_dap_event_stopped() {
        let msg = serde_json::json!({
            "type": "event",
            "event": "stopped",
            "body": {
                "threadId": 3,
                "reason": "breakpoint",
                "hitBreakpointIds": [10, 20],
            }
        });
        match parse_dap_event(&msg) {
            Some(DapEvent::Stopped {
                thread_id,
                reason,
                hit_breakpoint_ids,
            }) => {
                assert_eq!(thread_id, 3);
                assert_eq!(reason, "breakpoint");
                assert_eq!(hit_breakpoint_ids, vec![10, 20]);
            }
            other => panic!("expected Stopped, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_dap_event_continued() {
        let msg = serde_json::json!({
            "type": "event",
            "event": "continued",
            "body": { "threadId": 1 }
        });
        assert!(matches!(
            parse_dap_event(&msg),
            Some(DapEvent::Continued { thread_id: 1 })
        ));
    }

    #[test]
    fn test_parse_dap_event_exited() {
        let msg = serde_json::json!({
            "type": "event",
            "event": "exited",
            "body": { "exitCode": 137 }
        });
        assert!(matches!(
            parse_dap_event(&msg),
            Some(DapEvent::Exited { exit_code: 137 })
        ));
    }

    #[test]
    fn test_parse_dap_event_terminated_maps_to_exited() {
        let msg = serde_json::json!({ "type": "event", "event": "terminated" });
        assert!(matches!(
            parse_dap_event(&msg),
            Some(DapEvent::Exited { .. })
        ));
    }

    #[test]
    fn test_parse_dap_event_output() {
        let msg = serde_json::json!({
            "type": "event",
            "event": "output",
            "body": { "category": "stderr", "output": "error text\n" }
        });
        match parse_dap_event(&msg) {
            Some(DapEvent::Output { category, output }) => {
                assert_eq!(category, "stderr");
                assert_eq!(output, "error text\n");
            }
            other => panic!("expected Output, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_dap_event_unknown() {
        let msg = serde_json::json!({ "type": "event", "event": "thread" });
        // "thread" is a valid DAP event we don't handle — should return None
        assert!(parse_dap_event(&msg).is_none());
    }
}
