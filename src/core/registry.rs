//! Remote extension registry — fetch manifest list from GitHub and download scripts.
//! Also manages a local disk cache for offline/instant startup.

use std::path::{Path, PathBuf};

use super::extensions::ExtensionManifest;

/// Default registry URL — raw GitHub content from the vimcode-ext repo.
pub const DEFAULT_REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/JDonaghy/vimcode-ext/main/registry.json";

/// Derive the base URL for downloading extension files from a registry URL.
/// `".../main/registry.json"` → `".../main"`.
pub fn base_url_from_registry(registry_url: &str) -> String {
    registry_url
        .rsplit_once('/')
        .map(|(base, _)| base.to_string())
        .unwrap_or_else(|| registry_url.to_string())
}

// ─── Registry cache ───────────────────────────────────────────────────────────

/// Path to the local registry cache file.
fn cache_path() -> PathBuf {
    super::paths::vimcode_config_dir().join("registry_cache.json")
}

/// Load the cached registry from disk. Returns `None` on any I/O or parse error.
pub fn load_cache() -> Option<Vec<ExtensionManifest>> {
    let data = std::fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&data).ok()
}

/// Persist the registry to the local cache file (best-effort, silent on error).
pub fn save_cache(manifests: &[ExtensionManifest]) {
    if let Ok(json) = serde_json::to_string_pretty(manifests) {
        if let Some(parent) = cache_path().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(cache_path(), json);
    }
}

// ─── Remote fetch ─────────────────────────────────────────────────────────────

/// Fetch and deserialize the remote registry. Blocking — run in a background thread.
/// Returns `None` on any network or parse error so the caller degrades gracefully.
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
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("curl download failed"))
    }
}

/// Download a README from a registry. Returns `None` on failure or empty base URL.
pub fn fetch_readme(base_url: &str, ext_name: &str) -> Option<String> {
    if base_url.is_empty() {
        return None;
    }
    let url = format!("{}/{}/README.md", base_url, ext_name);
    let output = std::process::Command::new("curl")
        .args(["-sf", "--max-time", "10"])
        .arg(&url)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_url_is_nonempty() {
        assert!(!DEFAULT_REGISTRY_URL.is_empty());
        assert!(DEFAULT_REGISTRY_URL.starts_with("https://"));
    }

    #[test]
    fn base_url_from_registry_strips_filename() {
        assert_eq!(
            base_url_from_registry("https://example.com/main/registry.json"),
            "https://example.com/main"
        );
        assert_eq!(
            base_url_from_registry("https://example.com/registry.json"),
            "https://example.com"
        );
        // No slash — returns input as-is
        assert_eq!(base_url_from_registry("bare-string"), "bare-string");
    }

    #[test]
    fn cache_path_is_reasonable() {
        let p = cache_path();
        assert!(p.to_string_lossy().contains("registry_cache.json"));
    }
}
