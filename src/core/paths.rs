//! Cross-platform configuration directory helpers.
//!
//! All modules that need `~/.config/vimcode/` (or the platform equivalent)
//! should call [`vimcode_config_dir()`] instead of hardcoding paths.

use std::path::PathBuf;

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
