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
        // debugpy.adapter speaks DAP over stdio (like all other adapters here).
        // Do NOT use `python -m debugpy --listen PORT` — that is TCP-only and
        // requires a script argument; the adapter module handles launch requests.
        args: &["-m", "debugpy.adapter"],
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
    AdapterInfo {
        name: "netcoredbg",
        binary: "netcoredbg",
        args: &["--interpreter=vscode"],
        languages: &["csharp"],
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

/// Path to the editor-managed venv used exclusively for the debugpy adapter.
///
/// Using a dedicated venv avoids PEP 668 "externally managed environment"
/// errors that modern Linux distros (Ubuntu 22.04+, Debian 12, Fedora 38+)
/// produce when pip tries to install into the system Python.
pub fn debugpy_venv_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/vimcode/debugpy-venv"))
}

/// Find the Python interpreter for the *user's project* (not the adapter).
///
/// This is what debugpy uses to actually run the script being debugged.
/// Search order:
///   1. `$VIRTUAL_ENV/bin/python`          — explicitly activated venv
///   2. `<workspace>/venv/bin/python`      — conventional local venv name
///   3. `<workspace>/.venv/bin/python`     — PEP 582 / modern convention
///   4. `<workspace>/env/bin/python`       — older convention
///   5. `python3` on PATH                  — system / conda / pyenv
///   6. `python`  on PATH                  — last resort
#[allow(dead_code)]
pub fn find_project_python() -> Option<PathBuf> {
    find_project_python_in(None)
}

/// Same as [`find_project_python`] but also searches `workspace_root` for
/// common venv directories so the user doesn't need to activate manually.
pub fn find_project_python_in(workspace_root: Option<&std::path::Path>) -> Option<PathBuf> {
    // 1. Explicitly activated venv takes priority.
    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        #[cfg(target_os = "windows")]
        let p = PathBuf::from(&venv).join("Scripts").join("python.exe");
        #[cfg(not(target_os = "windows"))]
        let p = PathBuf::from(&venv).join("bin").join("python");
        if p.exists() {
            return Some(p);
        }
    }

    // 2-4. Common venv directory names relative to the workspace root.
    if let Some(root) = workspace_root {
        for venv_dir in &["venv", ".venv", "env"] {
            #[cfg(target_os = "windows")]
            let p = root.join(venv_dir).join("Scripts").join("python.exe");
            #[cfg(not(target_os = "windows"))]
            let p = root.join(venv_dir).join("bin").join("python");
            if p.exists() {
                return Some(p);
            }
        }
    }

    // 5-6. Fall back to whatever python3/python is on PATH.
    resolve_binary("python3").or_else(|| resolve_binary("python"))
}

/// Find the Python 3 executable for the debugpy adapter.
///
/// Search order:
///   1. `~/.config/vimcode/debugpy-venv/bin/python`  — managed venv (post-install)
///   2. `python3` on PATH                             — system Python 3
///   3. `python`  on PATH                             — last resort
pub fn find_python_binary() -> Option<PathBuf> {
    // Prefer the managed venv if it has already been created by :ExtInstall python.
    if let Some(venv) = debugpy_venv_dir() {
        #[cfg(target_os = "windows")]
        let venv_python = venv.join("Scripts").join("python.exe");
        #[cfg(not(target_os = "windows"))]
        let venv_python = venv.join("bin").join("python");
        if venv_python.exists() {
            return Some(venv_python);
        }
    }
    resolve_binary("python3").or_else(|| resolve_binary("python"))
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
            // Create a managed venv so we never touch the system Python (avoids
            // PEP 668 "externally managed environment" errors on Ubuntu 22.04+,
            // Debian 12, Fedora 38+, etc.).  After creation, install debugpy into
            // that venv.  On the next F5 press, find_python_binary() will discover
            // the venv python and launch the adapter from it.
            let system_python = find_python_binary()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "python3".to_string());
            let venv = debugpy_venv_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| "$HOME/.config/vimcode/debugpy-venv".to_string());
            #[cfg(target_os = "windows")]
            let venv_python = format!("{venv}\\Scripts\\python");
            #[cfg(not(target_os = "windows"))]
            let venv_python = format!("{venv}/bin/python");
            Some(format!(
                "{system_python} -m venv {venv} && {venv_python} -m pip install debugpy"
            ))
        }
        "delve" => Some("go install github.com/go-delve/delve/cmd/dlv@latest".to_string()),
        "netcoredbg" => Some(netcoredbg_install_cmd()),
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

fn netcoredbg_install_cmd() -> String {
    // netcoredbg releases: https://github.com/Samsung/netcoredbg/releases
    let arch = if std::env::consts::ARCH == "aarch64" {
        "arm64"
    } else {
        "amd64"
    };
    let os = if std::env::consts::OS == "macos" {
        "osx"
    } else {
        "linux"
    };
    format!(
        "curl -fSL 'https://github.com/Samsung/netcoredbg/releases/latest/download/\
         netcoredbg-{os}-{arch}.tar.gz' -o /tmp/vimcode-netcoredbg.tar.gz && \
         mkdir -p /tmp/vimcode-netcoredbg && \
         tar -xzf /tmp/vimcode-netcoredbg.tar.gz -C /tmp/vimcode-netcoredbg && \
         mkdir -p \"$HOME/.local/bin\" && \
         cp /tmp/vimcode-netcoredbg/netcoredbg/netcoredbg \"$HOME/.local/bin/netcoredbg\" && \
         chmod +x \"$HOME/.local/bin/netcoredbg\""
    )
}

// ---------------------------------------------------------------------------
// LaunchConfig — VSCode-compatible launch.json support
// ---------------------------------------------------------------------------

/// A single task definition parsed from `.vimcode/tasks.json` (VSCode-compatible).
#[derive(Debug, Clone)]
pub struct TaskDefinition {
    /// Human-readable label used to reference the task (e.g. from `preLaunchTask`).
    pub label: String,
    /// Task type: `"process"` or `"shell"`.
    pub task_type: String,
    /// The command to run.
    pub command: String,
    /// Arguments passed to the command.
    pub args: Vec<String>,
    /// Working directory for the task.
    pub cwd: String,
}

/// Parse the contents of a `tasks.json` file.
///
/// Returns an empty `Vec` on any parse failure so callers can gracefully degrade.
pub fn parse_tasks_json(content: &str, workspace_folder: &str) -> Vec<TaskDefinition> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(content) else {
        return Vec::new();
    };
    let Some(tasks) = json.get("tasks").and_then(|t| t.as_array()) else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for task in tasks {
        let label = task
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let task_type = task
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("shell")
            .to_string();
        let command = task
            .get("command")
            .and_then(|v| v.as_str())
            .map(|s| substitute_vars(s, workspace_folder))
            .unwrap_or_default();
        let args = task
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| substitute_vars(s, workspace_folder))
                    .collect()
            })
            .unwrap_or_default();
        let cwd = task
            .get("options")
            .and_then(|o| o.get("cwd"))
            .and_then(|v| v.as_str())
            .map(|s| substitute_vars(s, workspace_folder))
            .unwrap_or_else(|| workspace_folder.to_string());

        if !label.is_empty() && !command.is_empty() {
            result.push(TaskDefinition {
                label,
                task_type,
                command,
                args,
                cwd,
            });
        }
    }
    result
}

/// Convert a `TaskDefinition` into a shell command string suitable for `sh -c`.
pub fn task_to_shell_command(task: &TaskDefinition) -> String {
    if task.args.is_empty() {
        task.command.clone()
    } else {
        // For both "shell" and "process" types, join command + args.
        // Shell-quote args that contain spaces or special characters.
        let quoted_args: Vec<String> = task
            .args
            .iter()
            .map(|a| {
                if a.contains(' ') || a.contains('\'') || a.contains('"') || a.contains('\\') {
                    format!("'{}'", a.replace('\'', "'\\''"))
                } else {
                    a.clone()
                }
            })
            .collect();
        format!("{} {}", task.command, quoted_args.join(" "))
    }
}

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

/// Substitute `${workspaceFolder}` and `${workspaceFolderBasename}` in a string value.
fn substitute_vars(s: &str, workspace_folder: &str) -> String {
    let basename = std::path::Path::new(workspace_folder)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    s.replace("${workspaceFolder}", workspace_folder)
        .replace("${workspaceFolderBasename}", &basename)
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
        "coreclr" | "netcoredbg" | "csharp" => Some("netcoredbg"),
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
        "csharp" => r#"{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "coreclr",
      "request": "launch",
      "name": "Debug",
      "program": "${workspaceFolder}/bin/Debug/net8.0/${workspaceFolderBasename}.dll",
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
    // Glob-style markers checked via directory scan (e.g. *.sln for C#).
    const GLOB_EXTS: &[&str] = &["sln", "csproj"];
    let mut dir = start_dir.to_path_buf();
    loop {
        for marker in MARKERS {
            if dir.join(marker).exists() {
                return dir;
            }
        }
        // Check for glob-style markers (any file matching the extension).
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Some(ext) = entry.path().extension() {
                    let ext_s = ext.to_string_lossy();
                    if GLOB_EXTS.iter().any(|g| *g == ext_s.as_ref()) {
                        return dir;
                    }
                }
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

        // For debugpy, try python3 before python — most modern Linux distributions
        // ship Python 3 as `python3` without a `python` symlink.
        let binary = if info.name == "debugpy" {
            find_python_binary().ok_or_else(|| {
                "Python not found — install Python 3 and run :ExtInstall python".to_string()
            })?
        } else {
            resolve_binary(info.binary).ok_or_else(|| {
                format!(
                    "DAP binary '{}' not found (install via :DapInstall {name_or_lang})",
                    info.binary
                )
            })?
        };

        // For debugpy: verify the module is importable before spawning the TCP
        // server.  If it isn't, `spawn_tcp` would hang 2 s then show the opaque
        // "connection refused" error.  Better to fail fast with a clear message.
        if info.name == "debugpy" {
            let ok = std::process::Command::new(&binary)
                .args(["-c", "import debugpy"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !ok {
                return Err(format!(
                    "debugpy not installed for {} — run :ExtInstall python",
                    binary.display()
                ));
            }
        }

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
    fn test_install_cmd_debugpy_creates_venv() {
        // The install command must create a managed venv before installing into it.
        // This avoids PEP 668 "externally managed environment" errors.
        let cmd = install_cmd_for_adapter("debugpy").expect("debugpy should have install cmd");
        assert!(
            cmd.contains("venv"),
            "install cmd should create a venv: {cmd}"
        );
    }

    #[test]
    fn test_install_cmd_debugpy_installs_into_venv_not_system() {
        // debugpy must be installed into the managed venv, not the system Python.
        let cmd = install_cmd_for_adapter("debugpy").expect("debugpy should have install cmd");
        assert!(
            cmd.contains("debugpy-venv"),
            "install cmd should target the managed venv path: {cmd}"
        );
    }

    #[test]
    fn test_install_cmd_debugpy_uses_m_pip() {
        // Install via `-m pip` so the pip belongs to the venv python.
        let cmd = install_cmd_for_adapter("debugpy").expect("debugpy should have install cmd");
        assert!(
            cmd.contains("-m pip"),
            "install cmd should use `-m pip`: {cmd}"
        );
    }

    #[test]
    fn test_install_cmd_debugpy_does_not_use_bare_pip3() {
        // Must not start with a bare `pip`/`pip3` call — that would bypass the venv.
        let cmd = install_cmd_for_adapter("debugpy").expect("debugpy should have install cmd");
        assert!(
            !cmd.starts_with("pip"),
            "install cmd must not start with bare pip/pip3: {cmd}"
        );
    }

    #[test]
    fn test_debugpy_venv_dir_is_under_config_vimcode() {
        let dir = debugpy_venv_dir().expect("venv dir should be determinable");
        let s = dir.to_string_lossy();
        assert!(
            s.contains(".config") && s.contains("vimcode") && s.contains("debugpy-venv"),
            "debugpy venv dir should be ~/.config/vimcode/debugpy-venv: {s}"
        );
    }

    #[test]
    fn test_find_python_binary_returns_some_or_none_without_panic() {
        // Verifies the function doesn't panic regardless of what Python is installed.
        // If Python is present the path should contain "python"; if absent it's None.
        if let Some(path) = find_python_binary() {
            assert!(
                path.to_string_lossy().contains("python"),
                "found binary should contain 'python': {path:?}"
            );
        }
    }

    #[test]
    fn test_debugpy_adapter_binary_field_is_python() {
        // The registry `binary` field stays "python" — it drives the error message
        // path when find_python_binary() itself returns None.
        let info = DapManager::adapter_by_name("debugpy").expect("debugpy should be registered");
        assert_eq!(info.binary, "python");
    }

    #[test]
    fn test_debugpy_uses_stdio_not_tcp() {
        // debugpy.adapter speaks DAP over stdio, not TCP.
        let info = DapManager::adapter_by_name("debugpy").expect("debugpy registered");
        assert!(!info.use_tcp, "debugpy must use stdio (use_tcp=false)");
    }

    #[test]
    fn test_debugpy_args_use_adapter_module() {
        // Must use `python -m debugpy.adapter`, NOT `python -m debugpy --listen`.
        // The adapter module handles DAP over stdio and accepts launch requests.
        let info = DapManager::adapter_by_name("debugpy").expect("debugpy registered");
        assert!(
            info.args.contains(&"-m") && info.args.contains(&"debugpy.adapter"),
            "debugpy args must use debugpy.adapter: {:?}",
            info.args
        );
        assert!(
            !info.args.contains(&"--listen"),
            "debugpy must not use --listen (TCP mode): {:?}",
            info.args
        );
    }

    #[test]
    fn test_install_cmd_delve() {
        let cmd = install_cmd_for_adapter("delve");
        assert!(cmd.is_some());
        let cmd = cmd.unwrap();
        assert!(cmd.contains("go install") && cmd.contains("dlv"), "{cmd}");
    }

    #[test]
    fn test_install_cmd_netcoredbg() {
        let cmd = install_cmd_for_adapter("netcoredbg");
        assert!(cmd.is_some());
        let cmd = cmd.unwrap();
        assert!(cmd.contains("netcoredbg") && cmd.contains("curl"), "{cmd}");
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
        assert_eq!(type_to_adapter("coreclr"), Some("netcoredbg"));
        assert_eq!(type_to_adapter("netcoredbg"), Some("netcoredbg"));
        assert_eq!(type_to_adapter("csharp"), Some("netcoredbg"));
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

    #[test]
    fn test_generate_launch_json_csharp_contains_coreclr() {
        let json = generate_launch_json("csharp", "/proj");
        assert!(
            json.contains("coreclr"),
            "C# launch.json should use coreclr type"
        );
        assert!(json.contains("0.2.0"));
    }

    #[test]
    fn test_adapter_for_language_csharp() {
        let adapter = DapManager::adapter_for_language("csharp");
        assert!(adapter.is_some());
        assert_eq!(adapter.unwrap().name, "netcoredbg");
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

    // ── parse_tasks_json ────────────────────────────────────────────────────

    #[test]
    fn test_parse_tasks_json_basic() {
        let json = r#"{
            "version": "2.0.0",
            "tasks": [
                {
                    "label": "build",
                    "type": "shell",
                    "command": "cargo",
                    "args": ["build"]
                }
            ]
        }"#;
        let tasks = parse_tasks_json(json, "/home/user/project");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].label, "build");
        assert_eq!(tasks[0].task_type, "shell");
        assert_eq!(tasks[0].command, "cargo");
        assert_eq!(tasks[0].args, vec!["build"]);
        assert_eq!(tasks[0].cwd, "/home/user/project");
    }

    #[test]
    fn test_parse_tasks_json_with_workspace_vars() {
        let json = r#"{
            "version": "2.0.0",
            "tasks": [
                {
                    "label": "build",
                    "type": "process",
                    "command": "${workspaceFolder}/build.sh",
                    "args": ["--output", "${workspaceFolder}/dist"],
                    "options": {
                        "cwd": "${workspaceFolder}/src"
                    }
                }
            ]
        }"#;
        let tasks = parse_tasks_json(json, "/proj");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].command, "/proj/build.sh");
        assert_eq!(tasks[0].args, vec!["--output", "/proj/dist"]);
        assert_eq!(tasks[0].cwd, "/proj/src");
    }

    #[test]
    fn test_parse_tasks_json_multiple_tasks() {
        let json = r#"{
            "version": "2.0.0",
            "tasks": [
                {"label": "build", "command": "cargo", "args": ["build"]},
                {"label": "test", "command": "cargo", "args": ["test"]}
            ]
        }"#;
        let tasks = parse_tasks_json(json, "/");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].label, "build");
        assert_eq!(tasks[1].label, "test");
    }

    #[test]
    fn test_parse_tasks_json_skips_incomplete() {
        let json = r#"{
            "version": "2.0.0",
            "tasks": [
                {"label": "no-command"},
                {"command": "no-label"},
                {"label": "good", "command": "cargo"}
            ]
        }"#;
        let tasks = parse_tasks_json(json, "/");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].label, "good");
    }

    #[test]
    fn test_parse_tasks_json_invalid_returns_empty() {
        assert!(parse_tasks_json("not json", "/").is_empty());
        assert!(parse_tasks_json("{}", "/").is_empty());
        assert!(parse_tasks_json(r#"{"tasks": "bad"}"#, "/").is_empty());
    }

    // ── task_to_shell_command ────────────────────────────────────────────────

    #[test]
    fn test_task_to_shell_command_no_args() {
        let task = TaskDefinition {
            label: "build".into(),
            task_type: "shell".into(),
            command: "make".into(),
            args: vec![],
            cwd: "/".into(),
        };
        assert_eq!(task_to_shell_command(&task), "make");
    }

    #[test]
    fn test_task_to_shell_command_with_args() {
        let task = TaskDefinition {
            label: "build".into(),
            task_type: "shell".into(),
            command: "cargo".into(),
            args: vec!["build".into(), "--release".into()],
            cwd: "/".into(),
        };
        assert_eq!(task_to_shell_command(&task), "cargo build --release");
    }

    #[test]
    fn test_task_to_shell_command_quotes_spaces() {
        let task = TaskDefinition {
            label: "build".into(),
            task_type: "process".into(),
            command: "gcc".into(),
            args: vec!["-o".into(), "my program".into()],
            cwd: "/".into(),
        };
        assert_eq!(task_to_shell_command(&task), "gcc -o 'my program'");
    }
}
