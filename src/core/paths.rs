//! Cross-platform configuration directory helpers.
//!
//! All modules that need `~/.config/vimcode/` (or the platform equivalent)
//! should call [`vimcode_config_dir()`] instead of hardcoding paths.

use std::path::{Path, PathBuf};

/// Return the platform-appropriate VimCode configuration directory.
///
/// - **Linux / macOS**: `$HOME/.config/vimcode/`
/// - **Windows**: `%APPDATA%\vimcode\`  (fallback: `%USERPROFILE%\.config\vimcode\`)
pub fn vimcode_config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("vimcode");
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile).join(".config").join("vimcode");
        }
        PathBuf::from(".").join(".config").join("vimcode")
    }

    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config").join("vimcode")
    }
}

/// Return the user's home directory in a cross-platform way.
///
/// - **Linux / macOS**: `$HOME`
/// - **Windows**: `%USERPROFILE%` (fallback `%HOME%`)
pub fn home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
}

/// Return the platform-appropriate VimCode *data* directory — where
/// vimcode-managed tool acquisitions (#1345) are unpacked, as opposed to
/// [`vimcode_config_dir()`]'s user-editable settings/manifests.
///
/// - **Linux**: `$XDG_DATA_HOME/vimcode` (falls back to `$HOME/.local/share/vimcode`)
/// - **macOS**: `$HOME/Library/Application Support/vimcode`
/// - **Windows**: `%LOCALAPPDATA%\vimcode` (fallback: the config dir)
///
/// `VIMCODE_TEST_DATA_HOME` overrides this outright on every target — the
/// same pattern `lsp_manager::homebrew_prefixes()`'s
/// `VIMCODE_TEST_HOMEBREW_PREFIXES` uses (#917) — so tests can point tool
/// acquisition at a throwaway directory instead of a real user data dir.
pub fn vimcode_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VIMCODE_TEST_DATA_HOME") {
        return PathBuf::from(dir);
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("vimcode");
        }
        return vimcode_config_dir();
    }
    #[cfg(target_os = "macos")]
    {
        return home_dir()
            .join("Library")
            .join("Application Support")
            .join("vimcode");
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            return PathBuf::from(xdg).join("vimcode");
        }
        home_dir().join(".local").join("share").join("vimcode")
    }
}

/// Root directory for vimcode-managed tool acquisitions (#1345):
/// `<data dir>/tools`. Each tool gets `tools/<tool>/<version>/` plus a
/// `tools/<tool>/current` pointer file naming the active version.
pub fn managed_tools_dir() -> PathBuf {
    vimcode_data_dir().join("tools")
}

/// Directory for one managed tool, keyed by its binary name (e.g.
/// `terraform-ls`) — the same name `resolve_command`/`binary_on_path` look
/// up, so acquisition and resolution agree on where a tool lives without a
/// second name-mapping table.
pub fn managed_tool_dir(tool: &str) -> PathBuf {
    managed_tools_dir().join(tool)
}

/// Directory a specific version of a managed tool is unpacked into.
pub fn managed_tool_version_dir(tool: &str, version: &str) -> PathBuf {
    managed_tool_dir(tool).join(version)
}

/// Path to the "current version" pointer file for a managed tool. A plain
/// text file (not a symlink) so the same scheme works unprivileged on
/// Windows, where creating a symlink needs Developer Mode or admin rights.
pub fn managed_tool_current_pointer(tool: &str) -> PathBuf {
    managed_tool_dir(tool).join("current")
}

/// Read which version of `tool` is currently selected, if any.
pub fn managed_tool_current_version(tool: &str) -> Option<String> {
    let s = std::fs::read_to_string(managed_tool_current_pointer(tool)).ok()?;
    let v = s.trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// Point `tool`'s `current` pointer at `version`. Creates the tool's
/// directory if needed.
pub fn set_managed_tool_current(tool: &str, version: &str) -> std::io::Result<()> {
    let dir = managed_tool_dir(tool);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(managed_tool_current_pointer(tool), version)
}

/// Resolve `binary`'s absolute path under the managed tools dir, if any
/// version is currently selected there. This is the seam
/// `lsp_manager::resolve_command` probes first (#1345) — a vimcode-acquired
/// tool is found without any PATH/env changes.
pub fn managed_tool_binary_path(binary: &str) -> Option<PathBuf> {
    let version = managed_tool_current_version(binary)?;
    let dir = managed_tool_version_dir(binary, &version);
    let candidate = dir.join(binary);
    if candidate.is_file() {
        return Some(candidate);
    }
    #[cfg(target_os = "windows")]
    {
        let exe = dir.join(format!("{binary}.exe"));
        if exe.is_file() {
            return Some(exe);
        }
    }
    // The extracted file's name comes from `binary_path`'s basename, which
    // may differ from the resolver's `binary` key (e.g. a manifest's
    // `lsp.binary` short name vs. an archive entry named
    // `terraform-ls_1.2.3`). A managed-tool version directory holds exactly
    // one acquired binary, so fall back to "the one file in here".
    let mut entries = std::fs::read_dir(&dir).ok()?;
    entries.find_map(|e| {
        let path = e.ok()?.path();
        path.is_file().then_some(path)
    })
}

/// Strip the `\\?\` extended-length path prefix that Windows adds when
/// a path is canonicalized. Returns the path unchanged on non-Windows.
pub fn strip_unc_prefix(path: &Path) -> std::borrow::Cow<'_, Path> {
    #[cfg(target_os = "windows")]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return std::borrow::Cow::Owned(PathBuf::from(stripped));
        }
    }
    std::borrow::Cow::Borrowed(path)
}

/// Serializes every test *anywhere in this crate* that mutates the
/// process-global `VIMCODE_TEST_DATA_HOME` env var (read by
/// [`vimcode_data_dir`]) against every other one. Shared rather than a
/// per-module lock (this module's own tests, `tool_acquire.rs`'s tests, and
/// `tui_main/shell_app.rs`'s driver-tier tests all set this var) because
/// `cargo test` runs `#[test]`s in parallel threads within one process by
/// default — three independent, non-cooperating locks each guarding the
/// same global var is exactly the shape that raced in practice: a
/// `tool_acquire::tests` test observed a real `$HOME/.local/share/vimcode`
/// path instead of its own throwaway dir because a concurrently-running
/// `shell_app::tests` test (using its own, different lock) had already
/// restored the *previous* value by the time the first test's guard read
/// it back. See `crate::core::engine::terminal_ops::tests::HOME_ENV_LOCK`'s
/// doc comment for the same tradeoff applied to `HOME`/`PATH` instead.
#[cfg(test)]
pub(crate) static VIMCODE_TEST_DATA_HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_ends_with_vimcode() {
        let dir = vimcode_config_dir();
        let s = dir.to_string_lossy();
        assert!(
            s.ends_with("vimcode"),
            "config dir should end with 'vimcode': {s}"
        );
    }

    #[test]
    fn config_dir_is_not_empty() {
        let dir = vimcode_config_dir();
        assert!(dir.components().count() >= 2);
    }

    #[test]
    fn home_dir_is_not_empty() {
        let dir = home_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    // ── #1345: managed tools dir ────────────────────────────────────────────
    //
    // `VIMCODE_TEST_DATA_HOME` is process-global env state, mutated by tests
    // in this module, `tool_acquire.rs`, and `tui_main/shell_app.rs` alike —
    // see `super::VIMCODE_TEST_DATA_HOME_LOCK`'s doc comment for why they
    // all share one lock rather than each guarding their own.

    struct DataHomeGuard {
        old: Option<std::ffi::OsString>,
        dir: PathBuf,
    }

    impl DataHomeGuard {
        fn new(tag: &str) -> Self {
            let old = std::env::var_os("VIMCODE_TEST_DATA_HOME");
            let dir = std::env::temp_dir().join(format!(
                "vimcode_test_data_home_{tag}_{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::env::set_var("VIMCODE_TEST_DATA_HOME", &dir);
            Self { old, dir }
        }
    }

    impl Drop for DataHomeGuard {
        fn drop(&mut self) {
            match self.old.take() {
                Some(v) => std::env::set_var("VIMCODE_TEST_DATA_HOME", v),
                None => std::env::remove_var("VIMCODE_TEST_DATA_HOME"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn managed_tools_dir_is_under_data_dir() {
        let _lock = super::VIMCODE_TEST_DATA_HOME_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let guard = DataHomeGuard::new("layout");
        assert_eq!(managed_tools_dir(), guard.dir.join("tools"));
        assert_eq!(
            managed_tool_version_dir("terraform-ls", "0.32.0"),
            guard.dir.join("tools").join("terraform-ls").join("0.32.0")
        );
    }

    #[test]
    fn managed_tool_current_version_absent_by_default() {
        let _lock = super::VIMCODE_TEST_DATA_HOME_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("absent");
        assert_eq!(managed_tool_current_version("terraform-ls"), None);
        assert_eq!(managed_tool_binary_path("terraform-ls"), None);
    }

    #[test]
    fn set_managed_tool_current_then_binary_path_resolves() {
        let _lock = super::VIMCODE_TEST_DATA_HOME_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("resolve");

        let version_dir = managed_tool_version_dir("terraform-ls", "0.32.0");
        std::fs::create_dir_all(&version_dir).unwrap();
        std::fs::write(version_dir.join("terraform-ls"), b"fake binary").unwrap();

        set_managed_tool_current("terraform-ls", "0.32.0").unwrap();
        assert_eq!(
            managed_tool_current_version("terraform-ls"),
            Some("0.32.0".to_string())
        );
        assert_eq!(
            managed_tool_binary_path("terraform-ls"),
            Some(version_dir.join("terraform-ls"))
        );
    }

    #[test]
    fn managed_tool_binary_path_falls_back_to_sole_file_in_version_dir() {
        // The extracted filename can differ from the resolver's lookup key
        // (see the doc comment on `managed_tool_binary_path`) — cover that
        // fallback explicitly rather than only the exact-name-match path.
        let _lock = super::VIMCODE_TEST_DATA_HOME_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _guard = DataHomeGuard::new("fallback");

        let version_dir = managed_tool_version_dir("some-lsp", "1.0.0");
        std::fs::create_dir_all(&version_dir).unwrap();
        let odd_name = version_dir.join("some-lsp_1.0.0_linux_amd64");
        std::fs::write(&odd_name, b"fake binary").unwrap();

        set_managed_tool_current("some-lsp", "1.0.0").unwrap();
        assert_eq!(managed_tool_binary_path("some-lsp"), Some(odd_name));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn config_dir_unix_layout() {
        let dir = vimcode_config_dir();
        let s = dir.to_string_lossy();
        assert!(
            s.contains(".config/vimcode") || s.contains(".config\\vimcode"),
            "Unix config dir should contain .config/vimcode: {s}"
        );
    }
}
