//! Cross-platform configuration directory helpers.
//!
//! All modules that need `~/.config/vimcode/` (or the platform equivalent)
//! should call [`vimcode_config_dir()`] instead of hardcoding paths.

use std::path::{Path, PathBuf};

// Per-thread stand-in for `$HOME`, honoured by [`home_dir`] and
// [`vimcode_config_dir`] in `--lib` test builds only.
//
// # Why this exists instead of `std::env::set_var("HOME", …)` (#957 smoke)
//
// A handful of tests need vimcode to believe the home directory is a
// throwaway temp dir (a fake `~/.local/bin` to resolve a tool out of, an
// empty `~/.config/vimcode` so `Engine::new()` can't inherit the
// developer's real settings). Doing that with `set_var` is
// **process-global**, and the `--lib` test binary runs ~3.6k tests across
// as many threads as the machine has cores — so for the duration of one
// such test, *every other test* saw the fake `$HOME` too. Two concrete
// victims, both reproducible in roughly a third of full-suite runs and in
// none when run alone:
//
// * `TerminalSession::spawn` runs the developer's real `$SHELL` and
//   portable-pty snapshots the environment at fork time, so a PTY test on
//   another thread got a shell with no rc files in `$HOME`. That shell
//   still starts, but as a bare shell with different startup/line-editor
//   behaviour, and the command lines `Engine::terminal_run_command` /
//   `Engine::acp_launch_terminal_login` inject straight after spawn were
//   then processed differently — an ACP terminal-auth login pane exiting
//   instantly without running the login command, an extension-install
//   pane never reaching `is_exited()` so its finalize message never
//   painted.
// * `core::session::tests::test_workspace_session_path_is_stable` asserts
//   two successive `session_path_for_workspace` calls agree; a `$HOME`
//   flip between them is enough to break it.
//
// A thread-local has none of that reach: the override applies only to the
// thread that set it, so concurrent tests (and any background thread the
// engine spawns) keep seeing the real `$HOME`. Same mechanism, and same
// motivation, as `settings.rs`'s `TEST_SETTINGS_PATH_OVERRIDE`.
//
// Set it with [`set_test_home`]; the returned guard restores the previous
// value on drop.
#[cfg(test)]
thread_local! {
    static TEST_HOME_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// RAII guard returned by [`set_test_home`]: restores whatever override was
/// in effect on this thread before, so nested/sequential use is safe.
#[cfg(test)]
pub(crate) struct TestHomeGuard {
    previous: Option<PathBuf>,
}

#[cfg(test)]
impl Drop for TestHomeGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        TEST_HOME_OVERRIDE.with(|cell| *cell.borrow_mut() = previous);
    }
}

/// Make [`home_dir`] and [`vimcode_config_dir`] report `dir` as the home
/// directory **on this thread only**, until the returned guard drops.
///
/// See the `TEST_HOME_OVERRIDE` comment above for why tests must use this
/// rather than mutating the process's real `HOME`/`USERPROFILE`.
#[cfg(test)]
pub(crate) fn set_test_home(dir: &Path) -> TestHomeGuard {
    let previous = TEST_HOME_OVERRIDE.with(|cell| cell.borrow_mut().replace(dir.to_path_buf()));
    TestHomeGuard { previous }
}

/// The active thread-local home override, if any. Always `None` outside
/// `--lib` test builds.
#[cfg(test)]
fn test_home_override() -> Option<PathBuf> {
    TEST_HOME_OVERRIDE.with(|cell| cell.borrow().clone())
}

#[cfg(not(test))]
fn test_home_override() -> Option<PathBuf> {
    None
}

/// Return the platform-appropriate VimCode configuration directory.
///
/// - **Linux / macOS**: `$HOME/.config/vimcode/`
/// - **Windows**: `%APPDATA%\vimcode\`  (fallback: `%USERPROFILE%\.config\vimcode\`)
pub fn vimcode_config_dir() -> PathBuf {
    if let Some(home) = test_home_override() {
        return home.join(".config").join("vimcode");
    }

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
    if let Some(home) = test_home_override() {
        return home;
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
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
