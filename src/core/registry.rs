//! Remote extension registry — fetch manifest list from GitHub and download scripts.

use std::path::Path;

use super::extensions::ExtensionManifest;

/// Default registry URL — raw GitHub content (no GitHub Pages setup required).
/// Override via `settings.extension_registry_url` for self-hosted / custom domain.
pub const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/JDonaghy/vimcode/main/extensions/registry.json";

/// Base URL for downloading individual extension files (scripts, manifests).
pub const FILES_BASE_URL: &str =
    "https://raw.githubusercontent.com/JDonaghy/vimcode/main/extensions";

/// Fetch and deserialize the remote registry. Blocking — run in a background thread.
/// Returns `None` on any network or parse error so the caller degrades gracefully
/// to the BUNDLED offline fallback.
pub fn fetch_registry(url: &str) -> Option<Vec<ExtensionManifest>> {
    let output = std::process::Command::new("curl")
        .args(["-sf", "--max-time", "15", url])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice(&output.stdout).ok()
}

/// Download a single file from `url` to `dest` path via curl.
/// Creates the parent directory if it does not exist.
pub fn download_script(url: &str, dest: &Path) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = std::process::Command::new("curl")
        .args(["-sf", "--max-time", "30", "-o"])
        .arg(dest)
        .arg(url)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("curl download failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_url_is_nonempty() {
        assert!(!REGISTRY_URL.is_empty());
        assert!(REGISTRY_URL.starts_with("https://"));
    }

    #[test]
    fn files_base_url_is_nonempty() {
        assert!(!FILES_BASE_URL.is_empty());
    }
}
