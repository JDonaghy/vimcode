//! Remote extension registry — fetch manifest list from GitHub and download scripts.
//! Also manages a local disk cache for offline/instant startup.

use std::path::{Path, PathBuf};

use super::extensions::ExtensionManifest;

/// Default registry URL — raw GitHub content from the vimcode-ext repo.
/// Override via `settings.extension_registry_url` for self-hosted / custom domain.
pub const REGISTRY_URL: &str =
    "https://raw.githubusercontent.com/JDonaghy/vimcode-ext/main/registry.json";

/// Base URL for downloading individual extension files (scripts, manifests, READMEs).
pub const FILES_BASE_URL: &str = "https://raw.githubusercontent.com/JDonaghy/vimcode-ext/main";

// ─── Registry cache ───────────────────────────────────────────────────────────

/// Path to the local registry cache file.
fn cache_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config/vimcode/registry_cache.json")
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
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("curl download failed"))
    }
}

/// Download a README from the remote registry. Returns `None` on failure.
pub fn fetch_readme(ext_name: &str) -> Option<String> {
    let url = format!("{}/{}/README.md", FILES_BASE_URL, ext_name);
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
    fn registry_url_is_nonempty() {
        assert!(!REGISTRY_URL.is_empty());
        assert!(REGISTRY_URL.starts_with("https://"));
    }

    #[test]
    fn files_base_url_is_nonempty() {
        assert!(!FILES_BASE_URL.is_empty());
    }

    #[test]
    fn cache_path_is_reasonable() {
        let p = cache_path();
        assert!(p.to_string_lossy().contains("registry_cache.json"));
    }
}
