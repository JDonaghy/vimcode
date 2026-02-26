//! DAP adapter manager — new infrastructure module; methods used by engine (Session 84+).
#![allow(dead_code)]

use std::path::PathBuf;

use super::dap::DapServer;

// ---------------------------------------------------------------------------
// Built-in adapter registry
// ---------------------------------------------------------------------------

pub struct AdapterInfo {
    pub name: &'static str,
    /// Primary binary to look up on PATH / Mason bin.
    pub binary: &'static str,
    /// Arguments passed to the binary when spawning.
    pub args: &'static [&'static str],
    pub languages: &'static [&'static str],
    /// If true, the adapter communicates over a TCP socket: it prints
    /// "Listening on port N" to stdout and we connect to 127.0.0.1:N.
    /// If false, we use stdin/stdout for DAP messages directly.
    pub use_tcp: bool,
}

static ADAPTER_REGISTRY: &[AdapterInfo] = &[
    AdapterInfo {
        name: "codelldb",
        binary: "codelldb",
        // codelldb communicates over TCP. `spawn_tcp` replaces the "0"
        // with a free port it chose itself, then connects directly.
        args: &["--port", "0"],
        languages: &["rust", "c", "cpp"],
        use_tcp: true,
    },
    AdapterInfo {
        name: "debugpy",
        binary: "python",
        args: &["-m", "debugpy", "--listen", "0", "--wait-for-client"],
        languages: &["python"],
        use_tcp: false,
    },
    AdapterInfo {
        name: "delve",
        binary: "dlv",
        args: &["dap"],
        languages: &["go"],
        use_tcp: false,
    },
    AdapterInfo {
        name: "js-debug",
        binary: "node",
        args: &[],
        languages: &["javascript", "typescript"],
        use_tcp: false,
    },
    AdapterInfo {
        name: "java-debug",
        binary: "java-debug-adapter",
        args: &[],
        languages: &["java"],
        use_tcp: false,
    },
];

/// Return the Mason DAP binary directory (same path as LSP: `~/.local/share/nvim/mason/bin`).
fn mason_bin_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from)?;
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("HOME").map(PathBuf::from)?;

    #[cfg(target_os = "windows")]
    let dir = base.join("nvim-data").join("mason").join("bin");
    #[cfg(not(target_os = "windows"))]
    let dir = base
        .join(".local")
        .join("share")
        .join("nvim")
        .join("mason")
        .join("bin");

    if dir.is_dir() {
        Some(dir)
    } else {
        None
    }
}

/// Resolve a binary name to an absolute path.
/// Checks Mason bin directory first, then falls back to PATH via `which`/`where`.
pub fn resolve_binary(name: &str) -> Option<PathBuf> {
    if let Some(mason_bin) = mason_bin_dir() {
        let candidate = mason_bin.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    #[cfg(target_os = "windows")]
    let which_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let which_cmd = "which";

    let output = std::process::Command::new(which_cmd)
        .arg(name)
        .output()
        .ok()?;
    if output.status.success() {
        let path_str = String::from_utf8_lossy(&output.stdout);
        let first_line = path_str.lines().next()?.trim();
        if !first_line.is_empty() {
            return Some(PathBuf::from(first_line));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Install commands
// ---------------------------------------------------------------------------

/// Return a shell command that installs the named adapter, if one is known.
///
/// On Unix the command is run via `sh -c`; on Windows via `cmd /C`.
/// Returns `None` for adapters that require manual installation.
pub fn install_cmd_for_adapter(adapter_name: &str) -> Option<String> {
    match adapter_name {
        "codelldb" => Some(codelldb_install_cmd()),
        "debugpy" => {
            // debugpy is a Python package; `python` is already the binary we launch
            #[cfg(target_os = "windows")]
            return Some("pip install debugpy".to_string());
            #[cfg(not(target_os = "windows"))]
            Some("pip3 install debugpy".to_string())
        }
        "delve" => Some("go install github.com/go-delve/delve/cmd/dlv@latest".to_string()),
        // js-debug and java-debug require complex multi-step builds — no automated install
        _ => None,
    }
}

// Asset naming convention (verified against vadimcn/codelldb releases):
//   codelldb-{os}-{arch}.vsix
//   os:   linux | darwin | win32
//   arch: x64   | arm64
// Binary inside the VSIX: extension/adapter/codelldb (Linux/Mac)
//                          extension/adapter/codelldb.exe (Windows)

#[cfg(target_os = "windows")]
fn codelldb_install_cmd() -> String {
    // cmd /C runs this; inner PowerShell uses single-quoted strings to avoid cmd
    // escaping issues.  codelldb only ships x64 for Windows currently.
    concat!(
        "curl.exe -fSL https://github.com/vadimcn/codelldb/releases/latest/download/",
        "codelldb-win32-x64.vsix -o %TEMP%\\vimcode-codelldb.vsix",
        " && powershell -NoProfile -Command \"",
        "Expand-Archive $env:TEMP\\vimcode-codelldb.vsix $env:TEMP\\vimcode-codelldb -Force;",
        "$d=$env:USERPROFILE+'\\.local\\bin';",
        "New-Item -ItemType Directory -Force $d|Out-Null;",
        "Copy-Item $env:TEMP\\vimcode-codelldb\\extension\\adapter\\codelldb.exe $d\"",
    )
    .to_string()
}

#[cfg(not(target_os = "windows"))]
fn codelldb_install_cmd() -> String {
    // VS Code arch names: x86_64 → x64, aarch64 → arm64
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "x64"
    };
    // VS Code OS names: macos → darwin, linux → linux
    let os = if std::env::consts::OS == "macos" {
        "darwin"
    } else {
        "linux"
    };
    // Extract to an absolute temp dir (no `cd` needed).
    // codelldb requires its liblldb.so, lldb-server (for process launching on
    // Linux), and the lldb Python bindings at ~/.local/lldb/ (the path baked
    // into the binary at compile time).
    format!(
        "curl -fSL 'https://github.com/vadimcn/codelldb/releases/latest/download/\
         codelldb-{os}-{arch}.vsix' -o /tmp/vimcode-codelldb.vsix && \
         unzip -o /tmp/vimcode-codelldb.vsix \
           'extension/adapter/codelldb' \
           'extension/adapter/scripts/*' \
           'extension/lldb/bin/*' \
           'extension/lldb/lib/liblldb.so' \
           'extension/lldb/lib/libpython312.so' \
           'extension/lldb/lib/python3.12/*' \
           'extension/lldb/lib/lldb-python/*' \
           -d /tmp/vimcode-codelldb && \
         mkdir -p \"$HOME/.local/bin\" \
                  \"$HOME/.local/lldb/bin\" \
                  \"$HOME/.local/lldb/lib\" \
                  \"$HOME/.local/bin/scripts\" && \
         cp /tmp/vimcode-codelldb/extension/adapter/codelldb \"$HOME/.local/bin/codelldb\" && \
         cp -r /tmp/vimcode-codelldb/extension/adapter/scripts/. \"$HOME/.local/bin/scripts/\" && \
         cp -r /tmp/vimcode-codelldb/extension/lldb/bin/. \"$HOME/.local/lldb/bin/\" && \
         cp /tmp/vimcode-codelldb/extension/lldb/lib/liblldb.so \"$HOME/.local/lldb/lib/liblldb.so\" && \
         cp /tmp/vimcode-codelldb/extension/lldb/lib/libpython312.so \"$HOME/.local/lldb/lib/libpython312.so\" && \
         cp -r /tmp/vimcode-codelldb/extension/lldb/lib/python3.12 \"$HOME/.local/lldb/lib/\" && \
         cp -r /tmp/vimcode-codelldb/extension/lldb/lib/lldb-python/lldb \"$HOME/.local/lldb/lib/python3.12/\" && \
         chmod +x \"$HOME/.local/bin/codelldb\" \"$HOME/.local/lldb/bin/\"*"
    )
}

// ---------------------------------------------------------------------------
// LaunchConfig — VSCode-compatible launch.json support
// ---------------------------------------------------------------------------

/// A single debug configuration parsed from `.vscode/launch.json`.
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// Human-readable name shown in the UI.
    pub name: String,
    /// VSCode adapter type string (`"lldb"`, `"debugpy"`, `"go"`, …).
    pub adapter_type: String,
    /// `"launch"` or `"attach"`.
    pub request: String,
    /// Path to the program to debug (`${workspaceFolder}` already substituted).
    pub program: String,
    /// Command-line arguments passed to the program.
    pub args: Vec<String>,
    /// Working directory for the program.
    pub cwd: String,
    /// Original JSON object so extra fields (env, etc.) can be forwarded.
    pub raw: serde_json::Value,
}

/// Substitute `${workspaceFolder}` with `workspace_folder` in a string value.
fn substitute_vars(s: &str, workspace_folder: &str) -> String {
    s.replace("${workspaceFolder}", workspace_folder)
}

/// Parse the contents of a `.vscode/launch.json` file.
///
/// Returns an empty `Vec` on any parse failure so the caller can fall back to
/// the built-in hardcoded logic.
pub fn parse_launch_json(content: &str, workspace_folder: &str) -> Vec<LaunchConfig> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    let Some(configs) = json.get("configurations").and_then(|c| c.as_array()) else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for config in configs {
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Debug")
            .to_string();
        let adapter_type = config
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let request = config
            .get("request")
            .and_then(|v| v.as_str())
            .unwrap_or("launch")
            .to_string();
        let program = config
            .get("program")
            .and_then(|v| v.as_str())
            .map(|s| substitute_vars(s, workspace_folder))
            .unwrap_or_default();
        let args = config
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| substitute_vars(s, workspace_folder))
                    .collect()
            })
            .unwrap_or_default();
        let cwd = config
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(|s| substitute_vars(s, workspace_folder))
            .unwrap_or_else(|| workspace_folder.to_string());

        result.push(LaunchConfig {
            name,
            adapter_type,
            request,
            program,
            args,
            cwd,
            raw: config.clone(),
        });
    }
    result
}

/// Map a VSCode adapter type name to our internal adapter registry name.
///
/// Returns `None` for unknown type names.
pub fn type_to_adapter(adapter_type: &str) -> Option<&'static str> {
    match adapter_type {
        "lldb" | "codelldb" => Some("codelldb"),
        "debugpy" | "python" => Some("debugpy"),
        "go" | "delve" => Some("delve"),
        "node" | "chrome" | "pwa-node" | "pwa-chrome" => Some("js-debug"),
        "java" => Some("java-debug"),
        _ => None,
    }
}

/// Generate a VSCode-compatible `.vscode/launch.json` string for the given
/// language, substituting `${workspaceFolder}` with a literal `${workspaceFolder}`
/// (so the file stays portable).
pub fn generate_launch_json(lang: &str, workspace_folder: &str) -> String {
    match lang {
        "rust" => {
            // Detect the package name from Cargo.toml if possible.
            let pkg_name =
                detect_rust_package_name(workspace_folder).unwrap_or_else(|| "app".to_string());
            // Propagate flags from the current process so the debugged binary
            // runs in the same mode (e.g. --tui when the editor is in TUI mode).
            let inherited_args: Vec<String> = std::env::args()
                .skip(1)
                .filter(|a| a.starts_with('-'))
                .collect();
            let args_json = if inherited_args.is_empty() {
                "[]".to_string()
            } else {
                let quoted: Vec<String> =
                    inherited_args.iter().map(|a| format!("\"{a}\"")).collect();
                format!("[{}]", quoted.join(", "))
            };
            format!(
                r#"{{
  "version": "0.2.0",
  "configurations": [
    {{
      "type": "lldb",
      "request": "launch",
      "name": "Debug",
      "program": "${{workspaceFolder}}/target/debug/{pkg_name}",
      "args": {args_json},
      "cwd": "${{workspaceFolder}}",
      "sourceLanguages": ["rust"]
    }}
  ]
}}
"#
            )
        }
        "python" => r#"{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "debugpy",
      "request": "launch",
      "name": "Debug",
      "program": "${file}",
      "args": [],
      "cwd": "${workspaceFolder}"
    }
  ]
}
"#
        .to_string(),
        "go" => r#"{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "go",
      "request": "launch",
      "name": "Debug",
      "program": "${workspaceFolder}",
      "args": [],
      "cwd": "${workspaceFolder}"
    }
  ]
}
"#
        .to_string(),
        "javascript" | "typescript" => r#"{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "node",
      "request": "launch",
      "name": "Debug",
      "program": "${workspaceFolder}/index.js",
      "args": [],
      "cwd": "${workspaceFolder}"
    }
  ]
}
"#
        .to_string(),
        _ => {
            format!(
                r#"{{
  "version": "0.2.0",
  "configurations": [
    {{
      "type": "{lang}",
      "request": "launch",
      "name": "Debug",
      "program": "${{workspaceFolder}}/app",
      "args": [],
      "cwd": "${{workspaceFolder}}"
    }}
  ]
}}
"#
            )
        }
    }
}

/// Walk up from `start_dir` to find the nearest workspace root.
///
/// Workspace root markers (checked in order):
/// - `Cargo.toml`  (Rust)
/// - `package.json` (Node / JS / TS)
/// - `go.mod`       (Go)
/// - `pyproject.toml` / `setup.py` (Python)
/// - `.git`         (any VCS root — last resort)
///
/// Falls back to `start_dir` itself if no marker is found.
pub fn find_workspace_root(start_dir: &std::path::Path) -> std::path::PathBuf {
    const MARKERS: &[&str] = &[
        "Cargo.toml",
        "package.json",
        "go.mod",
        "pyproject.toml",
        "setup.py",
        ".git",
    ];
    let mut dir = start_dir.to_path_buf();
    loop {
        for marker in MARKERS {
            if dir.join(marker).exists() {
                return dir;
            }
        }
        match dir.parent() {
            Some(p) if p != dir => dir = p.to_path_buf(),
            _ => return start_dir.to_path_buf(),
        }
    }
}

/// Try to read the package name from `{workspace_folder}/Cargo.toml`.
fn detect_rust_package_name(workspace_folder: &str) -> Option<String> {
    let cargo_toml = std::path::Path::new(workspace_folder).join("Cargo.toml");
    let content = std::fs::read_to_string(cargo_toml).ok()?;
    // Simple line scan: find `name = "..."` under `[package]`
    let mut in_package = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[package]" {
            in_package = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_package = false;
        }
        if in_package && trimmed.starts_with("name") {
            if let Some(val) = trimmed.split_once('=').map(|x| x.1) {
                let name = val.trim().trim_matches('"').to_string();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// DapManager
// ---------------------------------------------------------------------------

pub struct DapManager {
    pub server: Option<DapServer>,
    pub adapter: Option<&'static AdapterInfo>,
}

impl DapManager {
    pub fn new() -> Self {
        Self {
            server: None,
            adapter: None,
        }
    }

    /// Find the registered adapter for a language identifier.
    pub fn adapter_for_language(lang: &str) -> Option<&'static AdapterInfo> {
        ADAPTER_REGISTRY
            .iter()
            .find(|a| a.languages.contains(&lang))
    }

    /// Find a registered adapter by exact name (e.g. "codelldb").
    pub fn adapter_by_name(name: &str) -> Option<&'static AdapterInfo> {
        ADAPTER_REGISTRY.iter().find(|a| a.name == name)
    }

    /// Start the adapter for `name_or_lang`. Tries exact adapter name first,
    /// then language identifier. Returns `Err` if nothing is registered.
    pub fn start_adapter(&mut self, name_or_lang: &str) -> Result<(), String> {
        let info = Self::adapter_by_name(name_or_lang)
            .or_else(|| Self::adapter_for_language(name_or_lang))
            .ok_or_else(|| format!("No DAP adapter registered for '{name_or_lang}'"))?;

        let binary = resolve_binary(info.binary).ok_or_else(|| {
            format!(
                "DAP binary '{}' not found (install via :DapInstall {name_or_lang})",
                info.binary
            )
        })?;

        let binary_str = binary.to_string_lossy().into_owned();
        let server = if info.use_tcp {
            DapServer::spawn_tcp(&binary_str, info.args)?
        } else {
            DapServer::spawn(&binary_str, info.args)?
        };
        self.server = Some(server);
        self.adapter = Some(info);
        Ok(())
    }

    /// Disconnect and drop the current session.
    pub fn stop(&mut self) {
        if let Some(mut server) = self.server.take() {
            server.disconnect();
        }
        self.adapter = None;
    }
}

impl Default for DapManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dap_adapter_registry_rust() {
        let info = DapManager::adapter_for_language("rust");
        assert!(info.is_some(), "should find adapter for 'rust'");
        assert_eq!(info.unwrap().name, "codelldb");
    }

    #[test]
    fn test_dap_adapter_registry_python() {
        let info = DapManager::adapter_for_language("python");
        assert!(info.is_some());
        assert_eq!(info.unwrap().name, "debugpy");
    }

    #[test]
    fn test_dap_adapter_registry_go() {
        let info = DapManager::adapter_for_language("go");
        assert!(info.is_some());
        assert_eq!(info.unwrap().name, "delve");
    }

    #[test]
    fn test_dap_adapter_registry_javascript() {
        let info = DapManager::adapter_for_language("javascript");
        assert!(info.is_some());
        assert_eq!(info.unwrap().name, "js-debug");
    }

    #[test]
    fn test_dap_adapter_registry_java() {
        let info = DapManager::adapter_for_language("java");
        assert!(info.is_some());
        assert_eq!(info.unwrap().name, "java-debug");
    }

    #[test]
    fn test_dap_adapter_registry_unknown() {
        let info = DapManager::adapter_for_language("cobol");
        assert!(info.is_none(), "unknown language should return None");
    }

    #[test]
    fn test_dap_resolve_binary_not_found() {
        let result = resolve_binary("__vimcode_nonexistent_dap_binary_xyzzy__");
        assert!(result.is_none(), "nonexistent binary should return None");
    }

    #[test]
    fn test_install_cmd_codelldb_contains_github_url() {
        let cmd = install_cmd_for_adapter("codelldb");
        assert!(cmd.is_some(), "codelldb should have an install command");
        let cmd = cmd.unwrap();
        assert!(
            cmd.contains("vadimcn/codelldb"),
            "install cmd should reference the codelldb GitHub repo: {cmd}"
        );
    }

    #[test]
    fn test_install_cmd_debugpy() {
        let cmd = install_cmd_for_adapter("debugpy");
        assert!(cmd.is_some());
        let cmd = cmd.unwrap();
        assert!(cmd.contains("pip") && cmd.contains("debugpy"), "{cmd}");
    }

    #[test]
    fn test_install_cmd_delve() {
        let cmd = install_cmd_for_adapter("delve");
        assert!(cmd.is_some());
        let cmd = cmd.unwrap();
        assert!(cmd.contains("go install") && cmd.contains("dlv"), "{cmd}");
    }

    #[test]
    fn test_install_cmd_unknown_returns_none() {
        assert!(install_cmd_for_adapter("java-debug").is_none());
        assert!(install_cmd_for_adapter("js-debug").is_none());
        assert!(install_cmd_for_adapter("nonexistent").is_none());
    }

    // ── parse_launch_json ─────────────────────────────────────────────────────

    #[test]
    fn test_parse_launch_json_basic() {
        let json = r#"{
            "version": "0.2.0",
            "configurations": [
                {
                    "type": "lldb",
                    "request": "launch",
                    "name": "Debug",
                    "program": "${workspaceFolder}/target/debug/myapp",
                    "args": [],
                    "cwd": "${workspaceFolder}"
                }
            ]
        }"#;
        let configs = parse_launch_json(json, "/home/user/myproject");
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "Debug");
        assert_eq!(configs[0].adapter_type, "lldb");
        assert_eq!(configs[0].request, "launch");
        assert_eq!(
            configs[0].program,
            "/home/user/myproject/target/debug/myapp"
        );
        assert_eq!(configs[0].cwd, "/home/user/myproject");
    }

    #[test]
    fn test_parse_launch_json_substitutes_workspace_folder() {
        let json = r#"{
            "version": "0.2.0",
            "configurations": [
                {
                    "type": "debugpy",
                    "request": "launch",
                    "name": "Python",
                    "program": "${workspaceFolder}/main.py",
                    "args": ["--verbose"],
                    "cwd": "${workspaceFolder}/src"
                }
            ]
        }"#;
        let configs = parse_launch_json(json, "/proj");
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].program, "/proj/main.py");
        assert_eq!(configs[0].cwd, "/proj/src");
        assert_eq!(configs[0].args, vec!["--verbose"]);
    }

    #[test]
    fn test_parse_launch_json_multiple_configs() {
        let json = r#"{
            "version": "0.2.0",
            "configurations": [
                {"type": "lldb", "request": "launch", "name": "A", "program": "a"},
                {"type": "go", "request": "launch", "name": "B", "program": "b"}
            ]
        }"#;
        let configs = parse_launch_json(json, "/");
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].name, "A");
        assert_eq!(configs[1].name, "B");
    }

    #[test]
    fn test_parse_launch_json_invalid_returns_empty() {
        assert!(parse_launch_json("not json", "/").is_empty());
        assert!(parse_launch_json("{}", "/").is_empty());
        assert!(parse_launch_json(r#"{"configurations": "bad"}"#, "/").is_empty());
    }

    // ── type_to_adapter ───────────────────────────────────────────────────────

    #[test]
    fn test_type_to_adapter_known_types() {
        assert_eq!(type_to_adapter("lldb"), Some("codelldb"));
        assert_eq!(type_to_adapter("codelldb"), Some("codelldb"));
        assert_eq!(type_to_adapter("debugpy"), Some("debugpy"));
        assert_eq!(type_to_adapter("python"), Some("debugpy"));
        assert_eq!(type_to_adapter("go"), Some("delve"));
        assert_eq!(type_to_adapter("delve"), Some("delve"));
        assert_eq!(type_to_adapter("node"), Some("js-debug"));
        assert_eq!(type_to_adapter("chrome"), Some("js-debug"));
        assert_eq!(type_to_adapter("pwa-node"), Some("js-debug"));
        assert_eq!(type_to_adapter("java"), Some("java-debug"));
    }

    #[test]
    fn test_type_to_adapter_unknown_returns_none() {
        assert_eq!(type_to_adapter("cobol"), None);
        assert_eq!(type_to_adapter(""), None);
        assert_eq!(type_to_adapter("rust"), None);
    }

    // ── generate_launch_json ──────────────────────────────────────────────────

    #[test]
    fn test_generate_launch_json_rust_contains_lldb() {
        let json = generate_launch_json("rust", "/proj");
        assert!(
            json.contains("\"lldb\""),
            "Rust launch.json should use lldb type"
        );
        assert!(json.contains("0.2.0"), "Should have version");
    }

    #[test]
    fn test_generate_launch_json_python_contains_debugpy() {
        let json = generate_launch_json("python", "/proj");
        assert!(json.contains("debugpy"));
    }

    // ── find_workspace_root ───────────────────────────────────────────────────

    #[test]
    fn test_find_workspace_root_finds_cargo_toml() {
        // Use the actual vimcode project root which has Cargo.toml.
        // Start from a subdirectory and expect to find the root.
        let start = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let root = find_workspace_root(&start);
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(root, manifest_dir, "should find project root from src/");
    }

    #[test]
    fn test_find_workspace_root_at_root_itself() {
        let start = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = find_workspace_root(start);
        assert_eq!(root, start, "root dir with Cargo.toml should return itself");
    }

    #[test]
    fn test_find_workspace_root_no_marker_returns_start() {
        // /tmp should have no workspace markers.
        let start = std::path::Path::new("/tmp");
        let root = find_workspace_root(start);
        assert_eq!(root, start, "no markers → fall back to start dir");
    }
}
